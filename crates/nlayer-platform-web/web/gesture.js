// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// Gesture gate and state machine for pan/pinch/rotation.
// Ported from nsh ca59368b web/nsh.js:5420-5480 (gate logic).
//
// Gate determines whether a pointer gesture becomes a pan or allows scroll.
// Single-axis pan bindings (only x or only y) check direction dominance.
// Dual-axis bindings activate pan on 6px threshold.

/**
 * Gate state machine:
 * 0 = undecided (waiting for movement to determine)
 * 1 = pan activated (gesture owns this pointer)
 * 2 = pan failed (scroll/browser owns this pointer)
 */
export class GestureGate {
  constructor() {
    this.pointers = new Map();
  }

  /**
   * Register a pointer down with its target chain.
   * @param {number} pointerId
   * @param {object} info - {x, y, pan, scroll, chain}
   *   pan: {id, panNodes: [xNode, yNode]}
   *   scroll: {id, horizontal}
   */
  begin(pointerId, info) {
    this.pointers.set(pointerId, {
      startX: info.x,
      startY: info.y,
      x: info.x,
      y: info.y,
      t0: performance.now(),
      pan: info.pan,
      scroll: info.scroll,
      gateState: 0, // 0=undecided, 1=activated, 2=failed
      mode: null, // null | "pan" | "scroll"
    });
  }

  /**
   * Update pointer position and decide gate.
   * Returns {consumed, phase, tx, ty, vx, vy, panId, preventDefault}
   * consumed: boolean - whether pan consumed this move
   * phase: 0=start, 1=move
   * preventDefault: whether to call event.preventDefault()
   */
  move(pointerId, x, y, now) {
    const p = this.pointers.get(pointerId);
    if (!p) return { consumed: false };

    p.x = x;
    p.y = y;
    const tx = x - p.startX;
    const ty = y - p.startY;

    // Decide gate (nsh.js:5461-5477 decideGate pattern)
    const gated = p.pan && p.scroll && p.gateState !== 2;
    if (gated && p.gateState === 0) {
      const panX = p.pan.panNodes[0] != null;
      const panY = p.pan.panNodes[1] != null;
      const horizontal = panX && !panY;
      const vertical = panY && !panX;

      if (horizontal || vertical) {
        // Single-axis: direction dominance
        const absTx = Math.abs(tx);
        const absTy = Math.abs(ty);

        if (Math.hypot(tx, ty) > 6) {
          if ((horizontal && absTx > absTy) || (vertical && absTy > absTx)) {
            // Pan wins (horizontal drag for x-only, vertical for y-only)
            p.gateState = 1;
          } else if (absTx > 6 || absTy > 6) {
            // Scroll wins (opposite direction)
            p.gateState = 2;
          }
        }
      } else {
        // Dual-axis: threshold-based
        if (Math.hypot(tx, ty) > 6) {
          p.gateState = 1;
        }
      }
    }

    const preventDefault = gated && p.gateState === 1;

    // Start pan if threshold crossed and not gated out
    if (!p.mode && Math.hypot(tx, ty) > 6) {
      if (p.pan && (!gated || p.gateState === 1)) {
        p.mode = "pan";
        if (!p.t0) p.t0 = now;
        return {
          consumed: true,
          phase: 0,
          tx: 0,
          ty: 0,
          vx: 0,
          vy: 0,
          panId: p.pan.id,
          preventDefault,
        };
      }
    }

    // Continue pan
    if (p.mode === "pan") {
      const dt = Math.max(1, now - p.t0);
      const vx = tx / dt;
      const vy = ty / dt;
      return {
        consumed: true,
        phase: 1,
        tx,
        ty,
        vx,
        vy,
        panId: p.pan.id,
        preventDefault: true,
      };
    }

    return { consumed: false, preventDefault };
  }

  /**
   * End pointer gesture.
   * Returns {phase, tx, ty, vx, vy, panId} for onPanEnd event
   */
  end(pointerId, now) {
    const p = this.pointers.get(pointerId);
    if (!p) return null;

    this.pointers.delete(pointerId);

    if (p.mode === "pan") {
      const tx = p.x - p.startX;
      const ty = p.y - p.startY;
      const dt = Math.max(1, now - p.t0);
      const vx = tx / dt;
      const vy = ty / dt;
      return {
        phase: 2, // end
        tx,
        ty,
        vx,
        vy,
        panId: p.pan.id,
      };
    }

    return null;
  }

  /**
   * Cancel pointer gesture (pointercancel).
   * Returns {phase, tx, ty, panId} for cancel event
   */
  cancel(pointerId) {
    const p = this.pointers.get(pointerId);
    if (!p) return null;

    this.pointers.delete(pointerId);

    if (p.mode === "pan") {
      const tx = p.x - p.startX;
      const ty = p.y - p.startY;
      return {
        phase: 3, // cancel
        tx,
        ty,
        vx: 0,
        vy: 0,
        panId: p.pan.id,
      };
    }

    return null;
  }

  /**
   * Check if pointer has an active pan gesture
   */
  hasPan(pointerId) {
    const p = this.pointers.get(pointerId);
    return p?.mode === "pan";
  }
}
