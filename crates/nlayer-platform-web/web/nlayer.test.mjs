// Player DOM mounting, screen/modal configs, and history tests

import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { Player } from "./nlayer.js";

function createTestEnv() {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>");
  const doc = dom.window.document;
  const root = doc.createElement("div");
  doc.body.appendChild(root);
  const host = {};
  const player = new Player(root, host, { baseCSS: "" });
  return { player, root, host, doc, window: dom.window };
}

test("Player creates screen DOM elements", () => {
  const { player } = createTestEnv();

  player.apply([["c", 2, "screen"]]);

  const el = player.view(2);
  assert.ok(el, "Should create screen element");
  assert.equal(el._kind, "screen");
  assert.ok(el.className.includes("nl-screen"));
});

test("Player creates modal DOM elements", () => {
  const { player } = createTestEnv();

  player.apply([["c", 2, "modal"]]);

  const el = player.view(2);
  assert.ok(el, "Should create modal element");
  assert.equal(el._kind, "modal");
  assert.ok(el.className.includes("nl-modal"));
});

test("Player creates stack DOM elements", () => {
  const { player } = createTestEnv();

  player.apply([["c", 2, "stack"]]);

  const el = player.view(2);
  assert.ok(el, "Should create stack element");
  assert.equal(el._kind, "stack");
  assert.ok(el.className.includes("nl-stack"));
});

test("Player creates headeritem DOM elements", () => {
  const { player } = createTestEnv();

  player.apply([["c", 2, "headeritem"]]);

  const el = player.view(2);
  assert.ok(el, "Should create headeritem element");
  assert.equal(el._kind, "headeritem");
  assert.ok(el.className.includes("nl-headeritem"));
});

test("Player applies screen config", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "screen"],
    [
      "scr",
      2,
      {
        headerHeight: 44,
        headerVisible: true,
        preventBack: false,
        animation: "default",
      },
    ],
  ]);

  const el = player.view(2);
  assert.ok(el._screenConfig);
  assert.equal(el._screenConfig.headerHeight, 44);
  assert.equal(el._screenConfig.headerVisible, true);
  assert.equal(el._screenConfig.animation, "default");
});

test("Player applies modal config", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "modal"],
    [
      "mc",
      2,
      {
        animation: "slide",
        presentation: "fullscreen",
        closeOnEscape: true,
        closeOnBackdropPress: true,
        closeOnSwipe: true,
      },
    ],
  ]);

  const el = player.view(2);
  assert.ok(el._modalConfig);
  assert.equal(el._modalConfig.animation, "slide");
  assert.equal(el._modalConfig.presentation, "fullscreen");
  assert.equal(el._modalConfig.closeOnEscape, true);
});

test("Player history depth counts screens in stacks", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "stack"],
    ["c", 3, "screen"],
    ["c", 4, "screen"],
    ["i", 2, 3, null],
    ["i", 2, 4, null],
    ["i", 1, 2, null],
  ]);

  const depth = player.backDepth();
  assert.equal(depth, 1, "Should count screens beyond first in stack");
});

test("Player history depth counts modals", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "modal"],
    ["i", 1, 2, null],
  ]);

  const depth = player.backDepth();
  assert.equal(depth, 1, "Should count modal as 1 depth");
});

test("Player history depth combines stacks and modals", () => {
  const { player } = createTestEnv();

  player.apply([
    ["c", 2, "stack"],
    ["c", 3, "screen"],
    ["c", 4, "screen"],
    ["c", 5, "screen"],
    ["i", 2, 3, null],
    ["i", 2, 4, null],
    ["i", 2, 5, null],
    ["i", 1, 2, null],
    ["c", 6, "modal"],
    ["i", 1, 6, null],
  ]);

  const depth = player.backDepth();
  assert.equal(depth, 3, "Should count 2 pushed screens + 1 modal");
});

test("Player history back forwards to engine for stack entries", () => {
  const { player, host, window } = createTestEnv();
  let backCallCount = 0;
  host.back = () => {
    backCallCount++;
  };
  player.apply([
    ["c", 2, "stack"],
    ["c", 3, "screen"],
    ["c", 4, "screen"],
    ["i", 2, 3, null],
    ["i", 2, 4, null],
    ["i", 1, 2, null],
  ]);
  player.syncHistory();
  assert.equal(player.historyDepth, 1);
  window.dispatchEvent(new window.PopStateEvent("popstate", { state: { nlayer: 0 } }));
  assert.equal(backCallCount, 1);
  assert.equal(player.historyDepth, 0);
});

test("Player screen no longer registers dismiss/backpress callbacks", () => {
  const { player, host, window } = createTestEnv();

  let dismissCalled = false;
  let backPressCalled = false;
  host.dismiss = () => {
    dismissCalled = true;
  };
  host.backPress = () => {
    backPressCalled = true;
  };

  player.apply([["c", 2, "screen"]]);

  const el = player.view(2);
  // Core dispatches events directly now, not through DOM events
  el.dispatchEvent(new window.CustomEvent("nlayer:dismiss"));
  assert.equal(dismissCalled, false, "Should not call dismiss via DOM event");

  el.dispatchEvent(new window.CustomEvent("nlayer:backpress"));
  assert.equal(backPressCalled, false, "Should not call backPress via DOM event");
});

test("Player modal no longer registers show/dismiss/requestclose callbacks", () => {
  const { player, host, window } = createTestEnv();

  let showCalled = false;
  let dismissCalled = false;
  let requestCloseCalled = false;

  host.show = () => {
    showCalled = true;
  };
  host.dismiss = () => {
    dismissCalled = true;
  };
  host.requestClose = (id, reason) => {
    requestCloseCalled = true;
  };

  player.apply([["c", 2, "modal"]]);

  const el = player.view(2);

  // Core dispatches events directly now, not through DOM events
  el.dispatchEvent(new window.CustomEvent("nlayer:show"));
  assert.equal(showCalled, false, "Should not call show via DOM event");

  el.dispatchEvent(new window.CustomEvent("nlayer:dismiss"));
  assert.equal(dismissCalled, false, "Should not call dismiss via DOM event");

  el.dispatchEvent(
    new window.CustomEvent("nlayer:requestclose", {
      detail: { reason: "cancel" },
    }),
  );
  assert.equal(requestCloseCalled, false, "Should not call requestClose via DOM event");
});

test("Player insets callback receives detail", () => {
  const { player, host, window } = createTestEnv();

  let receivedInsets = null;
  host.insets = (id, insets) => {
    receivedInsets = insets;
  };

  player.apply([["c", 2, "screen"]]);

  const el = player.view(2);
  el.dispatchEvent(
    new window.CustomEvent("nlayer:insets", {
      detail: { top: 50, right: 0, bottom: 30, left: 0 },
    }),
  );

  assert.ok(receivedInsets, "Should receive insets");
  assert.equal(receivedInsets.top, 50);
  assert.equal(receivedInsets.bottom, 30);
});

test("Only the last screen in a stack has nl-top class", () => {
  const { player, root } = createTestEnv();
  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
  ]);
  const stack = root.querySelector("[class*='nl-stack']");
  const screens = [...stack.children].filter((c) => c._kind === "screen");
  assert.equal(screens.length, 2);
  assert.equal(screens[0].classList.contains("nl-top"), false);
  assert.equal(screens[1].classList.contains("nl-top"), true);
});

test("Removing top screen makes previous screen top", () => {
  const { player, root } = createTestEnv();
  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
  ]);
  const stack = root.querySelector("[class*='nl-stack']");
  const screens = [...stack.children].filter((c) => c._kind === "screen");
  assert.equal(screens[1].classList.contains("nl-top"), true);
  assert.equal(player.backDepth(), 1);

  player.apply([["r", 12]]);
  assert.equal(screens[0].classList.contains("nl-top"), true);
  assert.equal(player.backDepth(), 0);
});

test("op_r removes DOM node and updates stack", () => {
  const { player, root } = createTestEnv();
  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
  ]);

  const stack = player.view(10);
  assert.equal(stack.children.length, 2, "Stack should have 2 screens");
  const screen12 = player.view(12);
  assert.ok(screen12, "Screen 12 should exist in views map");

  // Remove screen 12 with op "r" (removes from DOM, keeps in views map)
  player.apply([["r", 12]]);

  // Verify DOM node is removed from parent
  assert.equal(stack.children.length, 1, "Stack should have 1 screen after removal");
  assert.equal(screen12.parentNode, null, "Screen 12 should be detached from parent");
  assert.equal(player.view(12), screen12, "Screen 12 should still be in views map");

  // Verify previous screen becomes top
  const screen11 = player.view(11);
  assert.equal(screen11.classList.contains("nl-top"), true, "Screen 11 should be top");

  // Verify history depth drops
  assert.equal(player.backDepth(), 0, "Back depth should drop to 0");
});

test("a modal covers the window wherever the app rendered it", () => {
  const { player, root } = createTestEnv();
  player.host.setRootSize = () => {};
  player.apply([
    ["c", 2, "view"],
    ["i", 1, 2, null],
    ["c", 20, "modal"],
    ["i", 2, 20, null],
  ]);
  const modal = player.view(20);
  assert.equal(modal.parentNode, root, "the modal moves to the root layer");
  assert.equal(player.view(2).contains(modal), false);
});

test("Modal insert calls setRootSize with dimensions and insets", () => {
  const { player, root } = createTestEnv();
  const calls = [];
  player.host.setRootSize = (...args) => calls.push(args);
  player.safeArea = { top: 20, right: 0, bottom: 0, left: 0 };

  player.apply([
    ["c", 20, "modal"],
    ["i", 1, 20, null],
    ["mc", 20, {}],
  ]);

  assert.equal(calls.length, 1);
  const [id, width, height, top, right, bottom, left] = calls[0];
  assert.equal(id, 20);
  assert.equal(typeof width, "number");
  assert.equal(typeof height, "number");
  assert.equal(top, 20);
  assert.equal(right, 0);
  assert.equal(bottom, 0);
  assert.equal(left, 0);
});

test("Modal does not call setRootSize again if size unchanged", () => {
  const { player } = createTestEnv();
  const calls = [];
  player.host.setRootSize = (...args) => calls.push(args);
  player.safeArea = { top: 0, right: 0, bottom: 0, left: 0 };

  player.apply([
    ["c", 20, "modal"],
    ["i", 1, 20, null],
    ["mc", 20, {}],
  ]);

  assert.equal(calls.length, 1);

  player.apply([["mc", 20, {}]]);
  assert.equal(calls.length, 1); // No new call
});

test("App-initiated pop does not call host.back on popstate", async () => {
  const { player } = createTestEnv();
  const backCalls = [];
  player.host.back = () => backCalls.push(1);

  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
  ]);

  // Wait for async history sync
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(player.historyDepth, 1);

  // App pops the screen (syncHistory will set ignoredPops)
  player.apply([["r", 12]]);

  // Wait for async history sync
  await new Promise((resolve) => setTimeout(resolve, 10));

  // Simulate the resulting popstate event
  player.popState({
    state: { nlayer: 0, nlayerSession: player.historySession },
  });

  // ignoredPops should have consumed this, so host.back is not called
  assert.equal(backCalls.length, 0);
});

test("User back on stack calls host.back", async () => {
  const { player } = createTestEnv();
  const backCalls = [];
  player.host.back = () => backCalls.push(1);

  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
  ]);

  // Wait for async history sync
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(player.historyDepth, 1);

  // Simulate user pressing back button (not app-initiated)
  player.popState({
    state: { nlayer: 0, nlayerSession: player.historySession },
  });

  assert.equal(backCalls.length, 1);
  assert.equal(player.historyDepth, 0);
});

test("Foreign session back calls host.back for all entries", async () => {
  const { player } = createTestEnv();
  const backCalls = [];
  player.host.back = () => backCalls.push(1);

  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["c", 13, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["i", 10, 12, null],
    ["i", 10, 13, null],
  ]);

  // Wait for async history sync
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(player.historyDepth, 2);

  // Simulate back to a different session (e.g., after reload)
  player.popState({ state: { nlayer: 1, nlayerSession: "different" } });

  assert.equal(backCalls.length, 2); // Should call back twice
  assert.equal(player.historyDepth, 0);
});

test("Dispose removes event listeners", () => {
  const { player } = createTestEnv();

  // Listeners are set in constructor if available
  player.dispose();

  assert.equal(player.onPopState, null);
  assert.equal(player.onResize, null);
});
