// Gate logic tests for pan/scroll coordination

import assert from "node:assert/strict";
import test from "node:test";
import { GestureGate } from "./gesture.js";

test("GestureGate: single-axis x-only pan, horizontal drag activates pan", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, null] }, // x-only
    scroll: { id: 3, horizontal: false },
  });
  
  // Horizontal drag (dx=20, dy=5)
  const result = gate.move(1, 120, 105, performance.now());
  
  assert.equal(result.consumed, true, "Pan should consume horizontal drag");
  assert.equal(result.phase, 0, "Should be pan start");
  assert.equal(result.panId, 2);
});

test("GestureGate: single-axis x-only pan, vertical drag goes to scroll", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, null] }, // x-only
    scroll: { id: 3, horizontal: false },
  });
  
  // Vertical drag (dx=5, dy=20)
  const result = gate.move(1, 105, 120, performance.now());
  
  assert.equal(result.consumed, false, "Vertical drag should not be consumed by x-only pan");
});

test("GestureGate: single-axis y-only pan, vertical drag activates pan", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [null, 11] }, // y-only
    scroll: { id: 3, horizontal: true },
  });
  
  // Vertical drag (dx=5, dy=20)
  const result = gate.move(1, 105, 120, performance.now());
  
  assert.equal(result.consumed, true, "Pan should consume vertical drag");
  assert.equal(result.phase, 0, "Should be pan start");
});

test("GestureGate: single-axis y-only pan, horizontal drag goes to scroll", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [null, 11] }, // y-only
    scroll: { id: 3, horizontal: true },
  });
  
  // Horizontal drag (dx=20, dy=5)
  const result = gate.move(1, 120, 105, performance.now());
  
  assert.equal(result.consumed, false, "Horizontal drag should not be consumed by y-only pan");
});

test("GestureGate: dual-axis pan activates on 6px threshold", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] }, // both axes
    scroll: { id: 3, horizontal: false },
  });
  
  // Diagonal drag past threshold
  const result = gate.move(1, 105, 105, performance.now());
  
  assert.equal(result.consumed, true, "Dual-axis pan should activate");
  assert.equal(result.phase, 0);
});

test("GestureGate: pan reports velocity in correct units", () => {
  const gate = new GestureGate();
  const t0 = performance.now();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
  });
  
  // Start pan
  gate.move(1, 110, 110, t0 + 10);
  
  // Continue with known velocity
  const t1 = t0 + 110; // 100ms later
  const result = gate.move(1, 150, 130, t1);
  
  assert.equal(result.phase, 1, "Should be move phase");
  assert.equal(result.tx, 50);
  assert.equal(result.ty, 30);
  // velocity = displacement / time (points per millisecond)
  assert.ok(Math.abs(result.vx - 50/110) < 0.01, "vx should be displacement/time");
  assert.ok(Math.abs(result.vy - 30/110) < 0.01, "vy should be displacement/time");
});

test("GestureGate: end reports final velocity", () => {
  const gate = new GestureGate();
  const t0 = performance.now();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
  });
  
  gate.move(1, 110, 110, t0 + 10);
  gate.move(1, 150, 130, t0 + 110);
  
  const result = gate.end(1, t0 + 110);
  
  assert.equal(result.phase, 2, "Should be end phase");
  assert.equal(result.tx, 50);
  assert.equal(result.ty, 30);
  assert.ok(result.vx !== undefined);
  assert.ok(result.vy !== undefined);
});

test("GestureGate: cancel phase", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
  });
  
  gate.move(1, 110, 110, performance.now());
  
  const result = gate.cancel(1);
  
  assert.equal(result.phase, 3, "Should be cancel phase");
  assert.equal(result.vx, 0, "Cancel should have zero velocity");
  assert.equal(result.vy, 0);
});

test("GestureGate: no pan without binding", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: { id: 3, horizontal: false },
  });
  
  const result = gate.move(1, 120, 120, performance.now());
  
  assert.equal(result.consumed, false, "Should not consume without pan binding");
});

test("GestureGate: preventDefault flag set when pan activates", () => {
  const gate = new GestureGate();
  
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: { id: 3, horizontal: false },
  });
  
  const result = gate.move(1, 110, 110, performance.now());
  
  assert.equal(result.preventDefault, true, "Should set preventDefault when gated and activated");
});
