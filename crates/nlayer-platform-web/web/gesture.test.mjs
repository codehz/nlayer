// Gesture (pan/pinch/rotation) tests

import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { Player } from "./nlayer.js";

function createTestEnv() {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>");
  const doc = dom.window.document;
  const root = doc.createElement("div");
  doc.body.appendChild(root);
  const host = {
    pan: null,
    transformGesture: null,
  };
  const player = new Player(root, host, { baseCSS: "" });
  return { player, root, host, doc, window: dom.window };
}

test("Player handles pan binding", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "view"],
    ["pb", 2, 10, 11], // pan with x node 10, y node 11
  ]);

  const el = player.view(2);
  assert.ok(el._panNodes, "Should have pan nodes");
  assert.deepEqual(el._panNodes, [10, 11]);
});

test("Player handles pinch binding", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "view"],
    ["pinchb", 2, 12], // pinch with scale node 12
  ]);

  const el = player.view(2);
  assert.equal(el._pinchNode, 12, "Should have pinch node");
});

test("Player handles rotation binding", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "view"],
    ["rotb", 2, 13], // rotation with angle node 13
  ]);

  const el = player.view(2);
  assert.equal(el._rotationNode, 13, "Should have rotation node");
});

test("Player clears pan binding with null", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "view"],
    ["pb", 2, 10, 11],
    ["pb", 2, null, null], // clear
  ]);

  const el = player.view(2);
  assert.equal(el._panNodes, null, "Should clear pan nodes");
});

test("Pan gesture calls host.pan", () => {
  const { player, root, host } = createTestEnv();
  const calls = [];
  host.pan = (...args) => calls.push(args);

  player.apply([
    ["c", 2, "view"],
    ["f", 2, 10, 10, 100, 100],
    ["pb", 2, 10, 11],
  ]);

  const el = player.view(2);
  
  // Simulate pointer down
  const downEvent = new root.ownerDocument.defaultView.PointerEvent("pointerdown", {
    pointerId: 1,
    clientX: 50,
    clientY: 50,
    pointerType: "touch",
    button: 0,
  });
  Object.defineProperty(downEvent, "target", { value: el, writable: false });
  root.dispatchEvent(downEvent);

  // Simulate pointer move (should trigger pan start at >6px)
  const moveEvent = new root.ownerDocument.defaultView.PointerEvent("pointermove", {
    pointerId: 1,
    clientX: 60,
    clientY: 60,
    pointerType: "touch",
    button: 0,
  });
  Object.defineProperty(moveEvent, "target", { value: el, writable: false });
  root.dispatchEvent(moveEvent);

  assert.ok(calls.length >= 1, "Should call host.pan at least once");
  const startCall = calls[0];
  assert.equal(startCall[0], 2, "Should call with view id 2");
  assert.equal(startCall[1], 0, "Should call with phase 0 (start)");
});

test("Pan gesture reports translation and velocity", () => {
  const { player, root, host } = createTestEnv();
  const calls = [];
  host.pan = (...args) => calls.push(args);

  player.apply([
    ["c", 2, "view"],
    ["f", 2, 10, 10, 100, 100],
    ["pb", 2, 10, 11],
  ]);

  const el = player.view(2);
  
  // Down at 50,50
  const downEvent = new root.ownerDocument.defaultView.PointerEvent("pointerdown", {
    pointerId: 1,
    clientX: 50,
    clientY: 50,
    pointerType: "touch",
    button: 0,
  });
  Object.defineProperty(downEvent, "target", { value: el, writable: false });
  root.dispatchEvent(downEvent);

  // Move to 70,80 (dx=20, dy=30)
  const moveEvent = new root.ownerDocument.defaultView.PointerEvent("pointermove", {
    pointerId: 1,
    clientX: 70,
    clientY: 80,
    pointerType: "touch",
    button: 0,
  });
  Object.defineProperty(moveEvent, "target", { value: el, writable: false });
  root.dispatchEvent(moveEvent);

  // Up
  const upEvent = new root.ownerDocument.defaultView.PointerEvent("pointerup", {
    pointerId: 1,
    clientX: 70,
    clientY: 80,
    pointerType: "touch",
    button: 0,
  });
  Object.defineProperty(upEvent, "target", { value: el, writable: false });
  root.dispatchEvent(upEvent);

  const endCall = calls[calls.length - 1];
  assert.equal(endCall[1], 2, "End phase should be 2");
  assert.equal(endCall[2], 20, "Translation x should be 20");
  assert.equal(endCall[3], 30, "Translation y should be 30");
  // Velocity assertions would need timing control
});

test("Multiple gestures can coexist on same view", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "view"],
    ["pb", 2, 10, 11],
    ["pinchb", 2, 12],
    ["rotb", 2, 13],
  ]);

  const el = player.view(2);
  assert.deepEqual(el._panNodes, [10, 11]);
  assert.equal(el._pinchNode, 12);
  assert.equal(el._rotationNode, 13);
});
