// Gate logic tests for pan/scroll coordination and two-finger transforms

import assert from "node:assert/strict";
import test from "node:test";
import { GestureGate } from "./gesture.js";

test("GestureGate: single-axis x-only pan, horizontal drag activates pan", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, null] },
    scroll: { id: 3, horizontal: false },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 120, 105, performance.now());

  assert.equal(result.consumed, true);
  assert.equal(result.phase, 0);
  assert.equal(result.panId, 2);
});

test("GestureGate: single-axis x-only pan, vertical drag goes to scroll", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, null] },
    scroll: { id: 3, horizontal: false },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 105, 120, performance.now());

  assert.equal(result.consumed, false);
});

test("GestureGate: single-axis y-only pan, vertical drag activates pan", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [null, 11] },
    scroll: { id: 3, horizontal: true },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 105, 120, performance.now());

  assert.equal(result.consumed, true);
  assert.equal(result.phase, 0);
});

test("GestureGate: single-axis y-only pan, horizontal drag goes to scroll", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [null, 11] },
    scroll: { id: 3, horizontal: true },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 120, 105, performance.now());

  assert.equal(result.consumed, false);
});

test("GestureGate: dual-axis pan activates on 6px threshold", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: { id: 3, horizontal: false },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 105, 105, performance.now());

  assert.equal(result.consumed, true);
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
    pinch: null,
    rotation: null,
  });

  gate.move(1, 110, 110, t0 + 10);

  const t1 = t0 + 110;
  const result = gate.move(1, 150, 130, t1);

  assert.equal(result.phase, 1);
  assert.equal(result.tx, 50);
  assert.equal(result.ty, 30);
  assert.ok(Math.abs(result.vx - 50 / 110) < 0.01);
  assert.ok(Math.abs(result.vy - 30 / 110) < 0.01);
});

test("GestureGate: end reports final velocity", () => {
  const gate = new GestureGate();
  const t0 = performance.now();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
  });

  gate.move(1, 110, 110, t0 + 10);
  gate.move(1, 150, 130, t0 + 110);

  const result = gate.end(1, t0 + 110);

  assert.equal(result.phase, 2);
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
    pinch: null,
    rotation: null,
  });

  gate.move(1, 110, 110, performance.now());

  const result = gate.cancel(1);

  assert.equal(result.phase, 3);
  assert.equal(result.vx, 0);
  assert.equal(result.vy, 0);
});

test("GestureGate: no pan without binding", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: { id: 3, horizontal: false },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 120, 120, performance.now());

  assert.equal(result.consumed, false);
});

test("GestureGate: preventDefault flag set when pan activates", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: { id: 3, horizontal: false },
    pinch: null,
    rotation: null,
  });

  const result = gate.move(1, 110, 110, performance.now());

  assert.equal(result.preventDefault, true);
});

// Two-finger transform tests

test("GestureGate: two-finger pinch reports scale delta", () => {
  const gate = new GestureGate();

  // First finger
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 5 },
    rotation: null,
  });

  // Second finger (100px apart)
  gate.begin(2, {
    x: 200,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 5 },
    rotation: null,
  });

  // Move fingers apart (now 200px apart = 2x scale)
  gate.move(1, 50, 100, performance.now());
  const result = gate.move(2, 250, 100, performance.now());

  assert.ok(Array.isArray(result));
  const pinchResult = result.find((r) => r.rotation === false);
  assert.ok(pinchResult);
  assert.equal(pinchResult.transform, true);
  assert.equal(pinchResult.phase, 1);
  assert.equal(pinchResult.targetId, 5);
  assert.ok(Math.abs(pinchResult.amount - 2.0) < 0.01, "Scale should be ~2.0");
});

test("GestureGate: two-finger rotation reports angle delta", () => {
  const gate = new GestureGate();

  // Two fingers horizontal (angle = 0)
  gate.begin(1, { x: 100, y: 100, pan: null, scroll: null, pinch: null, rotation: { id: 6 } });
  gate.begin(2, { x: 200, y: 100, pan: null, scroll: null, pinch: null, rotation: { id: 6 } });

  // Rotate 90 degrees (second finger moves to vertical position)
  gate.move(1, 100, 100, performance.now());
  const result = gate.move(2, 100, 200, performance.now());

  assert.ok(Array.isArray(result));
  const rotResult = result.find((r) => r.rotation === true);
  assert.ok(rotResult);
  assert.equal(rotResult.transform, true);
  assert.equal(rotResult.phase, 1);
  assert.equal(rotResult.targetId, 6);
  assert.ok(Math.abs(rotResult.amount - 90) < 5, "Rotation should be ~90 degrees");
});

test("GestureGate: pinch and rotation can coexist", () => {
  const gate = new GestureGate();

  gate.begin(1, { x: 100, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: { id: 6 } });
  gate.begin(2, { x: 200, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: { id: 6 } });

  gate.move(1, 50, 100, performance.now());
  const result = gate.move(2, 250, 150, performance.now());

  assert.ok(Array.isArray(result));
  assert.equal(result.length, 2);
  assert.ok(result.some((r) => r.rotation === false && r.targetId === 5));
  assert.ok(result.some((r) => r.rotation === true && r.targetId === 6));
});

test("GestureGate: two-finger end reports final transform", () => {
  const gate = new GestureGate();

  gate.begin(1, { x: 100, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });
  gate.begin(2, { x: 200, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });

  gate.move(1, 50, 100, performance.now());
  gate.move(2, 250, 100, performance.now());

  const result = gate.end(1, performance.now());

  assert.ok(Array.isArray(result));
  const pinchEnd = result.find((r) => r.rotation === false);
  assert.equal(pinchEnd.phase, 2);
  assert.ok(pinchEnd.amount > 1.5);
});

test("GestureGate: three fingers does not crash", () => {
  const gate = new GestureGate();

  gate.begin(1, { x: 100, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });
  gate.begin(2, { x: 200, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });
  gate.begin(3, { x: 150, y: 150, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });

  const result = gate.move(1, 110, 110, performance.now());
  assert.ok(result !== undefined);
});

test("GestureGate: pan and transform do not interfere", () => {
  const gate = new GestureGate();

  // Start pan with one finger
  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
  });
  gate.move(1, 110, 110, performance.now());

  const panResult = gate.move(1, 120, 120, performance.now());
  assert.equal(panResult.consumed, true);
  assert.equal(panResult.panId, 2);

  // Add second finger for pinch (should not affect ongoing pan)
  gate.begin(2, { x: 200, y: 100, pan: null, scroll: null, pinch: { id: 5 }, rotation: null });

  // Continue pan
  const panContinue = gate.move(1, 130, 130, performance.now());
  assert.equal(panContinue.consumed, true);
});

// Animation value tracking tests

test("GestureGate: pan reads initial animation values and stops animations", () => {
  const gate = new GestureGate();
  const stopCalls = [];
  const animation = {
    value: (id) => (id === 10 ? 5 : id === 11 ? -3 : 0),
    stop: (id) => stopCalls.push(id),
    setValue: () => {},
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
    animation,
  });

  // Trigger pan start (first move past threshold)
  const result = gate.move(1, 120, 100, performance.now());

  assert.equal(result.consumed, true);
  assert.equal(result.phase, 0);
  assert.deepEqual(stopCalls, [10, 11]);
});

test("GestureGate: pan move updates animation values", () => {
  const gate = new GestureGate();
  const setValueCalls = [];
  const animation = {
    value: (id) => (id === 10 ? 5 : id === 11 ? -3 : 0),
    stop: () => {},
    setValue: (id, v) => setValueCalls.push({ id, v }),
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
    animation,
  });

  // Start pan (first move past threshold returns phase 0)
  const start = gate.move(1, 120, 100, performance.now());
  assert.equal(start.phase, 0);

  setValueCalls.length = 0;

  // Continue pan - should update values
  const move = gate.move(1, 150, 130, performance.now());
  assert.equal(move.phase, 1);
  assert.equal(setValueCalls.length, 2);
  assert.deepEqual(setValueCalls[0], { id: 10, v: 5 + 50 }); // startX + tx
  assert.deepEqual(setValueCalls[1], { id: 11, v: -3 + 30 }); // startY + ty
});

test("GestureGate: pan end includes current animation values", () => {
  const gate = new GestureGate();
  const animation = {
    value: (id) => (id === 10 ? 5 : id === 11 ? -3 : 0),
    stop: () => {},
    setValue: () => {},
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
    animation,
  });

  // Start and move
  gate.move(1, 110, 100, performance.now());
  gate.move(1, 120, 100, performance.now());
  gate.move(1, 150, 130, performance.now());

  // End
  const result = gate.end(1, performance.now());
  assert.equal(result.phase, 2);
  assert.equal(result.valueX, 5 + 50);
  assert.equal(result.valueY, -3 + 30);
});

test("GestureGate: pinch reads initial value and stops animation", () => {
  const gate = new GestureGate();
  const stopCalls = [];
  const animation = {
    value: (id) => (id === 20 ? 2.5 : 1.0),
    stop: (id) => stopCalls.push(id),
    setValue: () => {},
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  gate.begin(2, {
    x: 200,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  assert.deepEqual(stopCalls, [20]);
});

test("GestureGate: pinch move multiplies start value by scale", () => {
  const gate = new GestureGate();
  const setValueCalls = [];
  const animation = {
    value: (id) => (id === 20 ? 2.5 : 1.0),
    stop: () => {},
    setValue: (id, v) => setValueCalls.push({ id, v }),
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  gate.begin(2, {
    x: 200,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  setValueCalls.length = 0;

  // Move finger 1 from x=100 to x=50, distance changes from 100 to 150, scale = 1.5
  const result = gate.move(1, 50, 100, performance.now());
  assert.ok(Array.isArray(result));
  assert.equal(result[0].value, 2.5 * 1.5); // start * scale = 3.75
  assert.equal(setValueCalls.length, 1);
  assert.equal(setValueCalls[0].id, 20);
  assert.equal(setValueCalls[0].v, 3.75);
});

test("GestureGate: rotation adds angle to start value", () => {
  const gate = new GestureGate();
  const setValueCalls = [];
  const animation = {
    value: (id) => (id === 30 ? 45 : 0),
    stop: () => {},
    setValue: (id, v) => setValueCalls.push({ id, v }),
  };

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: null,
    pinch: null,
    rotation: { id: 4, node: 30 },
    animation,
  });

  gate.begin(2, {
    x: 200,
    y: 100,
    pan: null,
    scroll: null,
    pinch: null,
    rotation: { id: 4, node: 30 },
    animation,
  });

  setValueCalls.length = 0;

  // Move finger 1 to (150, 50), making vector (150,50) to (200,100)
  // dx=50, dy=50, new angle = atan2(50,50) = 45 degrees
  // Initial angle was atan2(0, 100) = 0, so rotation = 45 degrees
  const result = gate.move(1, 150, 50, performance.now());
  assert.ok(Array.isArray(result));
  const rotation = result[0];
  assert.equal(rotation.rotation, true);
  assert.ok(Math.abs(rotation.value - (45 + 45)) < 0.1); // start + angle
  assert.equal(setValueCalls.length, 1);
  assert.equal(setValueCalls[0].id, 30);
});
test("GestureGate: transform end includes velocity", () => {
  const gate = new GestureGate();
  const animation = {
    value: (id) => (id === 20 ? 1.0 : 0),
    stop: () => {},
    setValue: () => {},
  };

  const t0 = performance.now();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  gate.begin(2, {
    x: 200,
    y: 100,
    pan: null,
    scroll: null,
    pinch: { id: 3, node: 20 },
    rotation: null,
    animation,
  });

  // Move finger 1 from 100 to 50, distance 100->150, scale = 1.5 over 0.5 seconds
  gate.move(1, 50, 100, t0 + 500);

  const result = gate.end(1, t0 + 500);
  assert.ok(Array.isArray(result));
  const pinch = result[0];
  assert.equal(pinch.value, 1.5);
  // velocity = (1.5 - 1.0) / 0.5 = 1.0 per second
  assert.ok(Math.abs(pinch.velocity - 1.0) < 0.1);
});

test("GestureGate: single-axis horizontal pan, vertical drag does not consume", () => {
  const gate = new GestureGate();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, null] }, // x-only
    scroll: { id: 1, horizontal: false }, // vertical scroll available
    pinch: null,
    rotation: null,
  });

  // Vertical drag
  const result = gate.move(1, 100, 120, performance.now());
  assert.equal(result.consumed, false);
  assert.equal(result.preventDefault, false);
});

test("GestureGate: pan end returns correct translation after move", () => {
  const gate = new GestureGate();
  const now = performance.now();

  gate.begin(1, {
    x: 100,
    y: 100,
    pan: { id: 2, panNodes: [10, 11] },
    scroll: null,
    pinch: null,
    rotation: null,
  });

  // Move 80px horizontally (triggers pan mode at ~6px, then continues)
  gate.move(1, 180, 100, now + 10);

  // End
  const endResult = gate.end(1, now + 20);

  assert.equal(endResult.phase, 2);
  assert.equal(endResult.tx, 80);
  assert.equal(endResult.ty, 0);
  assert.equal(endResult.panId, 2);
});
