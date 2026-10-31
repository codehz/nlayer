// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// Gesture gate and state machine for pan/pinch/rotation.
// Ported from nsh ca59368b web/nsh.js:5420-5480 (gate logic) and
// two-finger transform tracking (nsh.js:5300-5360).

export class GestureGate {
  constructor() {
    this.pointers = new Map();
    this.twoFinger = null;
  }

  begin(pointerId, info) {
    this.pointers.set(pointerId, {
      id: pointerId,
      startX: info.x,
      startY: info.y,
      x: info.x,
      y: info.y,
      t0: performance.now(),
      pan: info.pan,
      scroll: info.scroll,
      pinch: info.pinch,
      rotation: info.rotation,
      gateState: 0,
      mode: null,
    });

    // Start two-finger transform (only if both pointers target same view)
    if (this.pointers.size === 2 && !this.twoFinger) {
      const [p1, p2] = [...this.pointers.values()];
      
      // Both must have pinch or rotation on same target
      const pinch1 = p1.pinch?.id;
      const pinch2 = p2.pinch?.id;
      const rot1 = p1.rotation?.id;
      const rot2 = p2.rotation?.id;
      
      const pinchId = (pinch1 && pinch2 && pinch1 === pinch2) ? pinch1 : null;
      const rotationId = (rot1 && rot2 && rot1 === rot2) ? rot1 : null;

      if (pinchId || rotationId) {
        const dx = p2.x - p1.x;
        const dy = p2.y - p1.y;
        this.twoFinger = {
          pointers: [p1.id, p2.id],
          dist: Math.hypot(dx, dy),
          angle: Math.atan2(dy, dx),
          pinchId,
          rotationId,
        };
      }
    }
  }

  move(pointerId, x, y, now) {
    const p = this.pointers.get(pointerId);
    if (!p) return { consumed: false };

    p.x = x;
    p.y = y;

    // Two-finger transform
    if (this.twoFinger && this.twoFinger.pointers.includes(pointerId)) {
      const [p1, p2] = this.twoFinger.pointers.map((id) => this.pointers.get(id));
      if (!p1 || !p2) return { consumed: false };

      const dx = p2.x - p1.x;
      const dy = p2.y - p1.y;
      const dist = Math.hypot(dx, dy);
      const angle = Math.atan2(dy, dx);

      const results = [];

      if (this.twoFinger.pinchId) {
        const scale = dist / Math.max(1, this.twoFinger.dist);
        results.push({
          transform: true,
          rotation: false,
          phase: 1,
          amount: scale,
          targetId: this.twoFinger.pinchId,
        });
      }

      if (this.twoFinger.rotationId) {
        const rotation = ((angle - this.twoFinger.angle) * 180) / Math.PI;
        results.push({
          transform: true,
          rotation: true,
          phase: 1,
          amount: rotation,
          targetId: this.twoFinger.rotationId,
        });
      }

      return results.length > 0 ? results : { consumed: false };
    }

    // Pan gate logic
    const tx = x - p.startX;
    const ty = y - p.startY;

    const gated = p.pan && p.scroll && p.gateState !== 2;
    if (gated && p.gateState === 0) {
      const panX = p.pan.panNodes[0] != null;
      const panY = p.pan.panNodes[1] != null;
      const horizontal = panX && !panY;
      const vertical = panY && !panX;

      if (horizontal || vertical) {
        const absTx = Math.abs(tx);
        const absTy = Math.abs(ty);

        if (Math.hypot(tx, ty) > 6) {
          if ((horizontal && absTx > absTy) || (vertical && absTy > absTx)) {
            p.gateState = 1;
          } else if (absTx > 6 || absTy > 6) {
            p.gateState = 2;
          }
        }
      } else {
        if (Math.hypot(tx, ty) > 6) {
          p.gateState = 1;
        }
      }
    }

    const preventDefault = gated && p.gateState === 1;

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

  end(pointerId, now) {
    const p = this.pointers.get(pointerId);
    if (!p) return null;

    // Capture transform state BEFORE deleting
    if (this.twoFinger && this.twoFinger.pointers.includes(pointerId)) {
      const [id1, id2] = this.twoFinger.pointers;
      const p1 = this.pointers.get(id1);
      const p2 = this.pointers.get(id2);
      const results = [];

      if (p1 && p2) {
        const dx = p2.x - p1.x;
        const dy = p2.y - p1.y;
        const dist = Math.hypot(dx, dy);
        const angle = Math.atan2(dy, dx);

        if (this.twoFinger.pinchId) {
          const scale = dist / Math.max(1, this.twoFinger.dist);
          results.push({
            transform: true,
            rotation: false,
            phase: 2,
            amount: scale,
            targetId: this.twoFinger.pinchId,
          });
        }

        if (this.twoFinger.rotationId) {
          const rotation = ((angle - this.twoFinger.angle) * 180) / Math.PI;
          results.push({
            transform: true,
            rotation: true,
            phase: 2,
            amount: rotation,
            targetId: this.twoFinger.rotationId,
          });
        }
      }

      this.twoFinger = null;
      this.pointers.clear();
      return results.length > 0 ? results : null;
    }

    this.pointers.delete(pointerId);

    // Pan end
    if (p.mode === "pan") {
      const tx = p.x - p.startX;
      const ty = p.y - p.startY;
      const dt = Math.max(1, now - p.t0);
      const vx = tx / dt;
      const vy = ty / dt;
      return {
        phase: 2,
        tx,
        ty,
        vx,
        vy,
        panId: p.pan.id,
      };
    }

    return null;
  }

  cancel(pointerId) {
    const p = this.pointers.get(pointerId);
    if (!p) return null;

    this.pointers.delete(pointerId);

    if (this.twoFinger && this.twoFinger.pointers.includes(pointerId)) {
      this.twoFinger = null;
      this.pointers.clear();
      return null;
    }

    if (p.mode === "pan") {
      const tx = p.x - p.startX;
      const ty = p.y - p.startY;
      return {
        phase: 3,
        tx,
        ty,
        vx: 0,
        vy: 0,
        panId: p.pan.id,
      };
    }

    return null;
  }

  hasPan(pointerId) {
    const p = this.pointers.get(pointerId);
    return p?.mode === "pan";
  }
}

/**
 * Attach gesture-aware pointer methods to a Player instance.
 */
export function attachGestures(player) {
  player.pointerDown = function(event) {
    this.swallowTarget = null;
    this.noteKeyboardTap(event);
    if (event.pointerType === "mouse" || event.button > 0) return;
    
    let pan = null, pinch = null, rotation = null;
    for (let el = event.target; el && el._id != null; el = el.parentNode) {
      if (!pan && el._panNodes) pan = { id: el._id, panNodes: el._panNodes };
      if (!pinch && el._pinchNode) pinch = { id: el._id };
      if (!rotation && el._rotationNode) rotation = { id: el._id };
    }
    
    const scrolls = this.scrollChain(event.target);
    const scroll = scrolls[0] ? { id: scrolls[0]._id, horizontal: !!scrolls[0]._horizontal } : null;
    
    if (!scrolls.length && !pan && !pinch && !rotation) return;
    if (event.pointerType !== "mouse" && scrolls.length) this.holding++;
    
    this.pointers.set(event.pointerId ?? 0, {
      x: event.clientX || 0,
      y: event.clientY || 0,
      startX: event.clientX || 0,
      startY: event.clientY || 0,
      scrolls,
    });
    
    this.gestures.begin(event.pointerId ?? 0, {
      x: event.clientX || 0,
      y: event.clientY || 0,
      pan,
      scroll,
      pinch,
      rotation,
    });
  };
  
  player.pointerMove = function(event) {
    const result = this.gestures.move(
      event.pointerId ?? 0,
      event.clientX || 0,
      event.clientY || 0,
      performance.now()
    );
    
    if (!result || !result.consumed) return;
    
    if (result.preventDefault) event.preventDefault();
    
    if (Array.isArray(result)) {
      for (const r of result) {
        this.host.transformGesture?.(r.targetId, r.rotation ? 1 : 0, r.phase, r.amount);
      }
    } else {
      this.host.pan?.(result.panId, result.phase, result.tx, result.ty, result.vx, result.vy);
    }
  };
  
  player.pointerUp = function(event) {
    const result = this.gestures.end(event.pointerId ?? 0, performance.now());
    
    if (!result) return;
    
    if (Array.isArray(result)) {
      for (const r of result) {
        this.host.transformGesture?.(r.targetId, r.rotation ? 1 : 0, r.phase, r.amount);
      }
    } else if (result.panId) {
      this.host.pan?.(result.panId, result.phase, result.tx, result.ty, result.vx, result.vy);
    }
  };
  
  player.pointerCancel = function(event) {
    if (event.pointerType === "mouse") return;
    
    const id = event.pointerId ?? 0;
    const tracked = this.pointers.get(id);
    this.pointers.delete(id);
    
    const result = this.gestures.cancel(id);
    if (result && result.panId) {
      this.host.pan?.(result.panId, result.phase, result.tx, result.ty, result.vx, result.vy);
    }
    
    const scrolls = tracked?.scrolls ?? this.scrollChain(event.target);
    if (!scrolls.length) return;
    const dx = (event.clientX || 0) - (tracked?.x || 0);
    const dy = (event.clientY || 0) - (tracked?.y || 0);
    const horizontal = Math.abs(dx) > Math.abs(dy);
    const el = scrolls.find((view) => !!view._horizontal === horizontal) ?? scrolls[0];
    this.beginDrag(el);
  };
}
