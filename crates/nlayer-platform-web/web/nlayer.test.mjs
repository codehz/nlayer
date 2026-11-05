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
  assert.equal(el.tagName, "DIALOG");
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

test("Screen places only the header slot and covers the top inset", () => {
  const { player } = createTestEnv();
  const calls = [];
  player.host.setRootSize = (...args) => calls.push(args);
  player.safeArea = { top: 22, right: 8, bottom: 14, left: 10 };

  player.apply([
    ["c", 2, "stack"],
    ["c", 3, "screen"],
    ["c", 4, "headeritem"],
    ["c", 5, "headeritem"],
    ["i", 1, 2, null],
    ["i", 2, 3, null],
    ["i", 3, 4, null],
    ["i", 3, 5, null],
    ["f", 2, 0, 0, 390, 800],
    [
      "scr",
      3,
      {
        headerHeight: 44,
        headerVisible: true,
        headerView: 4,
        left: 5,
        animation: "default",
      },
    ],
  ]);

  const screen = player.view(3);
  assert.equal(player.view(4).parentNode, screen);
  assert.equal(player.view(5).parentNode, null);
  const reported = calls.filter((call) => call[0] === 3);
  const last = reported.at(-1);
  assert.ok(last, "screen reports its size");
  assert.deepEqual(last.slice(1), [390, 800, 0, 8, 14, 10]);
});

test("Screen without a header reports the window safe area", () => {
  const { player } = createTestEnv();
  const calls = [];
  player.host.setRootSize = (...args) => calls.push(args);
  player.safeArea = { top: 22, right: 8, bottom: 14, left: 10 };

  player.apply([
    ["c", 2, "stack"],
    ["c", 3, "screen"],
    ["i", 1, 2, null],
    ["i", 2, 3, null],
    ["f", 3, 0, 0, 390, 800],
  ]);

  const last = calls.filter((call) => call[0] === 3).at(-1);
  assert.ok(last);
  assert.deepEqual(last.slice(1), [390, 800, 22, 8, 14, 10]);
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

test("Player presents a modal with showModal and routes Escape to back", () => {
  const { player, host, window } = createTestEnv();
  let backs = 0;
  host.back = () => {
    backs += 1;
  };
  let shown = 0;
  window.HTMLDialogElement.prototype.showModal = function showModal() {
    shown += 1;
    this.open = true;
  };

  player.apply([
    ["c", 2, "modal"],
    ["i", 1, 2, null],
  ]);

  const el = player.view(2);
  assert.equal(shown, 1, "Should open the native dialog");
  assert.equal(el.open, true);

  const cancel = new window.Event("cancel", { cancelable: true });
  el.dispatchEvent(cancel);
  assert.equal(cancel.defaultPrevented, true, "Should keep the dialog open");
  assert.equal(backs, 1, "Escape should be a back");
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

function stubAnimate(window) {
  const started = [];
  window.Element.prototype.animate = function animate(keyframes, options) {
    const record = { el: this, keyframes, options, onfinish: null };
    started.push(record);
    return record;
  };
  return started;
}

function stackOf(player, animation = "default") {
  player.apply([
    ["c", 10, "stack"],
    ["c", 11, "screen"],
    ["c", 12, "screen"],
    ["i", 1, 10, null],
    ["i", 10, 11, null],
    ["f", 10, 0, 0, 390, 800],
    ["scr", 11, { animation }],
    ["scr", 12, { animation }],
  ]);
}

test("push slides the new screen in and parallaxes the one under it", () => {
  const { player, window } = createTestEnv();
  const started = stubAnimate(window);
  stackOf(player);
  player.apply([["i", 10, 12, null]]);
  const top = player.view(12);
  const under = player.view(11);
  assert.equal(top.classList.contains("nl-top"), true);
  assert.equal(under.classList.contains("nl-shown"), true);
  assert.equal(under.style.transform, "translateX(-30%)");
  assert.deepEqual(started[0].keyframes, [
    { transform: "translateX(100%)" },
    { transform: "translateX(0%)" },
  ]);
  assert.equal(started[0].options.duration, 300);
  assert.deepEqual(started[1].keyframes, [
    { transform: "translateX(0%)" },
    { transform: "translateX(-30%)" },
  ]);
});

test("pop keeps the leaving screen until the slide ends", () => {
  const { player, window } = createTestEnv();
  const started = stubAnimate(window);
  stackOf(player);
  player.apply([["i", 10, 12, null]]);
  started.length = 0;
  const leaving = player.view(12);
  player.apply([["r", 12]]);
  assert.equal(leaving.parentNode, player.view(10), "pixels stay until the slide ends");
  assert.equal(player.backDepth(), 0, "history drops when the route pops");
  assert.equal(leaving.style.pointerEvents, "none");
  assert.equal(started[0].options.fill, "forwards");
  assert.equal(started[0].options.duration, 300);
  started[0].onfinish();
  assert.equal(leaving.parentNode, null);
  assert.equal(player.view(11).classList.contains("nl-top"), true);
});

test("fade and reduced motion do not slide", () => {
  const { player, window } = createTestEnv();
  const started = stubAnimate(window);
  stackOf(player, "fade");
  player.apply([["i", 10, 12, null]]);
  assert.deepEqual(started[0].keyframes, [{ opacity: 0 }, { opacity: 1 }]);
  assert.equal(started[0].options.duration, 200);
  assert.equal(started.length, 1);

  started.length = 0;
  window.matchMedia = (query) => ({ matches: query.includes("reduce") });
  player.apply([
    ["c", 13, "screen"],
    ["scr", 13, { animation: "default" }],
    ["i", 10, 13, null],
  ]);
  assert.deepEqual(started.at(-1).keyframes, [{ opacity: 0 }, { opacity: 1 }]);
});

test("animation none removes the screen at once", () => {
  const { player, window } = createTestEnv();
  const started = stubAnimate(window);
  stackOf(player, "none");
  player.apply([["i", 10, 12, null]]);
  assert.equal(started.length, 0);
  player.apply([["r", 12]]);
  assert.equal(player.view(12).parentNode, null);
  assert.equal(started.length, 0);
});

test("popToRoot animates only the old top", () => {
  const { player, window } = createTestEnv();
  const started = stubAnimate(window);
  stackOf(player);
  player.apply([
    ["c", 13, "screen"],
    ["scr", 13, { animation: "default" }],
    ["i", 10, 12, null],
    ["i", 10, 13, null],
  ]);
  started.length = 0;
  player.apply([
    ["r", 13],
    ["r", 12],
  ]);
  assert.equal(player.view(12).parentNode, null);
  assert.equal(player.view(13).parentNode, player.view(10));
  assert.equal(started.length, 2);
  started[0].onfinish();
  assert.equal(player.view(13).parentNode, null);
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

test("a dialog popup lets presses through to the backdrop", () => {
  const { player } = createTestEnv();
  player.host.setRootSize = () => {};
  player.apply([
    ["c", 20, "modal"],
    ["i", 1, 20, null],
    ["c", 21, "view"],
    ["c", 22, "view"],
    ["c", 23, "view"],
    ["i", 20, 21, null],
    ["i", 20, 22, null],
    ["i", 22, 23, null],
  ]);
  assert.equal(player.view(21).style.pointerEvents, "", "backdrop keeps presses");
  assert.equal(player.view(22).style.pointerEvents, "none", "popup does not cover the backdrop");
  assert.equal(player.view(23).style.pointerEvents, "auto", "the card still takes presses");

  player.apply([["r", 21]]);
  assert.equal(player.view(22).style.pointerEvents, "", "popup takes presses once it is alone");
  assert.equal(player.view(23).style.pointerEvents, "", "the card drops the explicit auto");
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

/** Replaces the page clock so a test can finish a modal's exit. */
function holdTimers(player) {
  const queued = [];
  player.window.setTimeout = (fn, ms) => {
    queued.push({ fn, ms });
    return queued.length;
  };
  player.window.clearTimeout = (id) => {
    const item = queued[id - 1];
    if (item) item.fn = null;
  };
  return queued;
}

function mountModal(player, animation) {
  player.host.setRootSize = () => {};
  player.apply([
    ["c", 20, "modal"],
    ["c", 21, "view"],
    ["i", 1, 20, null],
    ["i", 20, 21, null],
    ["f", 20, 0, 0, 100, 400],
    ["mc", 20, { animation }],
  ]);
  return { modal: player.view(20), child: player.view(21) };
}

test("a removed modal keeps its content until the exit motion ends", () => {
  const { player, root } = createTestEnv();
  const queued = holdTimers(player);
  const { modal, child } = mountModal(player, "slide");
  // The child is released first. It still has to ride the modal out.
  player.apply([
    ["d", 21],
    ["d", 20],
  ]);
  assert.equal(player.view(20), undefined, "the element is already gone from the app");
  assert.equal(modal.isConnected, true, "the layer stays up for the exit");
  assert.equal(child.parentNode, modal, "content removed with the modal stays in it");
  assert.equal(modal.style.transform, "translateY(400px)");
  assert.equal(modal.style.pointerEvents, "none");
  assert.equal(player.backDepth(), 0, "a leaving modal is not a history entry");
  assert.equal(root.contains(modal), true);
  const leave = queued.find((item) => item.ms === 350);
  assert.ok(leave, "slide exits over 350ms");
  leave.fn();
  assert.equal(modal.isConnected, false);
  assert.equal(child.isConnected, false);
});

test("a fade modal exits by opacity and none waits only a turn", () => {
  const { player } = createTestEnv();
  const queued = holdTimers(player);
  const faded = mountModal(player, "fade");
  player.apply([
    ["d", 21],
    ["d", 20],
  ]);
  assert.equal(faded.modal.style.opacity, "0");
  assert.equal(faded.modal.style.transform, "");
  assert.equal(faded.child.parentNode, faded.modal);
  queued.find((item) => item.ms === 250).fn();
  assert.equal(faded.modal.isConnected, false);

  const none = mountModal(player, "none");
  const before = queued.length;
  player.apply([
    ["d", 21],
    ["d", 20],
  ]);
  assert.equal(none.modal.isConnected, true);
  assert.equal(none.modal.style.opacity, "");
  assert.equal(none.child.parentNode, none.modal);
  const leave = queued.slice(before).find((item) => item.ms === 0 && item.fn);
  assert.ok(leave, "none leaves on the next turn");
  leave.fn();
  assert.equal(none.modal.isConnected, false);
});
test("a fade modal enters from transparent", () => {
  const { player } = createTestEnv();
  const frames = [];
  player.window.requestAnimationFrame = (fn) => {
    frames.push(fn);
    return frames.length;
  };
  const { modal } = mountModal(player, "fade");
  assert.equal(modal.style.opacity, "0");
  assert.equal(modal.style.transition, "none");
  assert.equal(frames.length, 1);
  frames[0]();
  assert.equal(frames.length, 2);
  frames[1]();
  assert.equal(modal.style.opacity, "");
  assert.equal(modal.style.transition, "");
  assert.equal(modal._entering, false);
});

test("inserting a modal again cancels an exit that has not finished", () => {
  const { player } = createTestEnv();
  holdTimers(player);
  const { modal } = mountModal(player, "slide");
  player.apply([["r", 20]]);
  assert.equal(modal._leaving, true);
  assert.equal(modal.style.transform, "translateY(400px)");
  player.apply([["i", 1, 20, null]]);
  assert.equal(modal._leaving, false);
  assert.equal(modal.style.transform, "translateY(400px)");
  assert.equal(modal.style.transition, "none");
  assert.equal(modal.style.pointerEvents, "");
  assert.equal(modal.isConnected, true);
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

test("Pan event reports translation, velocity, and values in correct units", () => {
  const { player, doc } = createTestEnv();
  const panCalls = [];
  player.host.pan = (...args) => panCalls.push(args);

  // Create view with pan binding
  player.apply([
    ["c", 2, "view"],
    ["i", 1, 2, null],
    ["op_xf", 2, [["translateX", 100]]],
  ]);

  const el = player.view(2);
  el._panNodes = [100, 101];

  // Mock animation
  player.animation = {
    value: (id) => (id === 100 ? 10 : id === 101 ? 20 : 0),
    stop: () => {},
    setValue: () => {},
  };

  // Simulate pan gesture
  const downEvent = new doc.defaultView.PointerEvent("pointerdown", {
    clientX: 100,
    clientY: 150,
    pointerId: 1,
  });
  Object.defineProperty(downEvent, "target", { value: el });
  player.pointerDown(downEvent);

  // Move to trigger pan start
  player.pointerMove(
    new doc.defaultView.PointerEvent("pointermove", {
      clientX: 110,
      clientY: 150,
      pointerId: 1,
    }),
  );

  player.pointerMove(
    new doc.defaultView.PointerEvent("pointermove", {
      clientX: 150,
      clientY: 180,
      pointerId: 1,
    }),
  );

  // End gesture
  player.pointerUp(
    new doc.defaultView.PointerEvent("pointerup", {
      clientX: 150,
      clientY: 180,
      pointerId: 1,
    }),
  );

  // Find the end event (phase 2)
  const endCall = panCalls.find((args) => args[1] === 2);
  assert.ok(endCall, "Should have pan end event");

  const [id, phase, tx, ty, vx, vy, valueX, valueY] = endCall;
  assert.equal(id, 2);
  assert.equal(phase, 2);
  assert.equal(tx, 50); // CSS pixels
  assert.equal(ty, 30); // CSS pixels
  // velocity is in points/ms, should be non-zero
  assert.ok(typeof vx === "number");
  assert.ok(typeof vy === "number");
  // values should be start + displacement
  assert.equal(valueX, 10 + 50);
  assert.equal(valueY, 20 + 30);
});

test("Single-axis horizontal pan does not produce vertical scroll", () => {
  const { player, doc } = createTestEnv();

  // Create scrollable view and pan view
  player.apply([
    ["c", 2, "scroll"],
    ["c", 3, "view"],
    ["i", 1, 2, null],
    ["i", 2, 3, null],
  ]);

  const scroll = player.view(2);
  const view = player.view(3);
  scroll._horizontal = false; // vertical scroll
  view._panNodes = [100, null]; // x-only pan

  // Simulate vertical drag
  const downEvent = new doc.defaultView.PointerEvent("pointerdown", {
    clientX: 100,
    clientY: 100,
    pointerId: 1,
  });
  Object.defineProperty(downEvent, "target", { value: view });
  player.pointerDown(downEvent);

  // Move vertically (should not consume for x-only pan)
  const moveEvent = new doc.defaultView.PointerEvent("pointermove", {
    clientX: 100,
    clientY: 150,
    pointerId: 1,
  });
  let preventDefaultCalled = false;
  moveEvent.preventDefault = () => {
    preventDefaultCalled = true;
  };

  player.pointerMove(moveEvent);

  // Should not prevent default (scroll should happen)
  assert.equal(preventDefaultCalled, false);
});

test("Single-axis horizontal pan activates on horizontal drag", () => {
  const { player, doc } = createTestEnv();
  const panCalls = [];
  player.host.pan = (...args) => panCalls.push(args);

  // Create scrollable view and pan view
  player.apply([
    ["c", 2, "scroll"],
    ["c", 3, "view"],
    ["i", 1, 2, null],
    ["i", 2, 3, null],
  ]);

  const scroll = player.view(2);
  const view = player.view(3);
  scroll._horizontal = false; // vertical scroll
  view._panNodes = [100, null]; // x-only pan

  player.animation = {
    value: () => 0,
    stop: () => {},
    setValue: () => {},
  };

  // Simulate horizontal drag
  const downEvent = new doc.defaultView.PointerEvent("pointerdown", {
    clientX: 100,
    clientY: 100,
    pointerId: 1,
  });
  Object.defineProperty(downEvent, "target", { value: view });
  player.pointerDown(downEvent);

  // Move horizontally
  player.pointerMove(
    new doc.defaultView.PointerEvent("pointermove", {
      clientX: 150,
      clientY: 100,
      pointerId: 1,
    }),
  );

  // Should activate pan
  assert.ok(panCalls.length > 0, "Should call pan");
  const startCall = panCalls.find((args) => args[1] === 0);
  assert.ok(startCall, "Should have pan start event");
});

test("Scroll view with snapToOffsets calls host.snap on scrollEnded", () => {
  const { player, doc } = createTestEnv();
  let snapCalls = [];
  player.host.snap = (...args) => {
    snapCalls.push(args);
    return 100; // Return snap target
  };

  // Create scroll view
  player.apply([
    ["c", 2, "scroll"],
    ["i", 1, 2, null],
    ["op_sc", 2, false, false, 0], // vertical scroll
  ]);

  const el = player.view(2);
  assert.ok(el, "Element should exist");

  // Manually set snap config (simulating what op_scs would do)
  el._snapConfig = {
    paging: false,
    snapInterval: 0,
    snapOffsets: [0, 100, 200, 300],
    snapAlignment: 0,
    disableIntervalMomentum: false,
    deceleration: 0.998,
  };
  el._horizontal = false;

  // Simulate scroll ending at offset 50
  Object.defineProperty(el, "scrollTop", { value: 50, writable: true });
  Object.defineProperty(el, "clientHeight", { value: 400, writable: true });
  Object.defineProperty(el, "scrollHeight", { value: 1000, writable: true });
  let scrollToCalled = false;
  let scrollToOpts = null;
  el.scrollTo = function (opts) {
    scrollToCalled = true;
    scrollToOpts = opts;
  };

  player.scrollEnded(el);

  assert.ok(snapCalls.length > 0, "Should call host.snap");
  const [offset, velocity, viewport, content, paging, snapInterval, snapOffsets] = snapCalls[0];
  assert.equal(offset, 50);
  assert.equal(velocity, 0);
  assert.deepEqual(snapOffsets, [0, 100, 200, 300]);
  assert.ok(scrollToCalled, "Should call scrollTo with snap target");
  if (scrollToCalled) {
    assert.equal(scrollToOpts.top, 100);
    assert.equal(scrollToOpts.behavior, "smooth");
  }
});

test("a text carries its selectable state, selection color and hrefs", () => {
  const { player } = createTestEnv();
  player.apply([
    ["c", 6, "text"],
    ["f", 6, 0, 0, 100, 20],
    ["i", 1, 6, null],
    [
      "t",
      6,
      0,
      0,
      100,
      20,
      [[0, 0, "Terms", 0, 0, ""]],
      [],
      [],
      [],
      true,
      "#ff0000",
      [[0, 5, 7]],
      [],
      "Terms",
    ],
    ["c", 7, "text"],
    ["i", 1, 7, null],
    ["t", 7, 0, 20, 100, 20, [[0, 0, "label", 0, 0, ""]], [], [], [], false, null, [], [], "label"],
    ["c", 8, "text"],
    ["i", 1, 8, null],
    ["t", 8, 0, 40, 100, 20, [[0, 0, "auto", 0, 0, ""]], [], [], [], null, null, [], [], "auto"],
  ]);
  const selectable = player.view(6);
  assert.equal(selectable._selectable, true);
  assert.ok(selectable.classList.contains("nl-selectable"));
  assert.ok(!selectable.classList.contains("nl-noselect"));
  assert.equal(selectable.style.getPropertyValue("--nl-selection"), "#ff0000");
  assert.deepEqual(selectable._hrefs, [[0, 5, 7]]);

  const off = player.view(7);
  assert.equal(off._noselect, true);
  assert.ok(off.classList.contains("nl-noselect"));
  assert.ok(!off.classList.contains("nl-selectable"));
  assert.equal(off.style.getPropertyValue("--nl-selection"), "");

  const auto = player.view(8);
  assert.equal(auto._selectable, false);
  assert.equal(auto._noselect, false);
  assert.ok(!auto.classList.contains("nl-selectable"));
  assert.ok(!auto.classList.contains("nl-noselect"));
});

test("op_sel marks a selectable view's subtree", () => {
  const { player } = createTestEnv();
  player.apply([
    ["c", 4, "view"],
    ["i", 1, 4, null],
    ["c", 6, "text"],
    ["i", 4, 6, null],
    ["t", 6, 0, 0, 100, 20, [[0, 0, "A", 0, 0, ""]], [], [], [], null, null, [], [], "A"],
    ["sel", 4, true],
  ]);
  const area = player.view(4);
  assert.equal(area._area, true);
  assert.ok(area.classList.contains("nl-area"));
  assert.equal(player.selectArea(player.view(6)), area);
  assert.equal(player.selectableText(player.view(6)), true);
  player.apply([["sel", 4, false]]);
  assert.equal(area._area, false);
  assert.ok(!area.classList.contains("nl-area"));
  assert.equal(player.selectableText(player.view(6)), false);
});

test("a short press on an href goes to the core press with its node", () => {
  const { player, window } = createTestEnv();
  const presses = [];
  player.host.press = (id) => presses.push(id);
  player.apply([
    ["c", 6, "text"],
    ["f", 6, 0, 0, 100, 20],
    ["i", 1, 6, null],
    ["e", 6, ["onPress"]],
    [
      "t",
      6,
      0,
      0,
      100,
      20,
      [[0, 0, "Terms", 0, 0, ""]],
      [],
      [],
      [[0, 5, 9]],
      null,
      null,
      [[0, 5, 7]],
      [],
      "Terms",
    ],
  ]);
  const el = player.view(6);
  const click = (offset) => {
    const event = new window.MouseEvent("click", { bubbles: true });
    event._utf16 = offset;
    el.dispatchEvent(event);
  };
  click(2);
  assert.deepEqual(presses, [7], "the innermost href's node, not the block");
  click(8);
  assert.deepEqual(presses, [7, 6], "a press outside the href still reports");
});

test("a click after a selection reports nothing", () => {
  const { player, window } = createTestEnv();
  const presses = [];
  player.host.press = (id) => presses.push(id);
  player.apply([
    ["c", 6, "text"],
    ["i", 1, 6, null],
    ["e", 6, ["onPress"]],
    [
      "t",
      6,
      0,
      0,
      100,
      20,
      [[0, 0, "Terms", 0, 0, ""]],
      [],
      [],
      [],
      true,
      null,
      [[0, 5, 7]],
      [],
      "Terms",
    ],
  ]);
  const el = player.view(6);
  el._text.dispatchEvent(new window.Event("selectstart"));
  assert.equal(el._selected, true);
  const event = new window.MouseEvent("click", { bubbles: true });
  event._utf16 = 2;
  el.dispatchEvent(event);
  assert.deepEqual(presses, [], "a selection is not a press");
  assert.equal(el._selected, false);
});

test("a long press on selectable text is a selection, not onLongPress", async () => {
  const { player, window } = createTestEnv();
  const long = [];
  player.host.longPress = (id) => long.push(id);
  player.apply([
    ["c", 6, "text"],
    ["i", 1, 6, null],
    ["e", 6, ["onLongPress"]],
    ["lp", 6, 5],
    ["t", 6, 0, 0, 100, 20, [[0, 0, "Terms", 0, 0, ""]], [], [], [], true, null, [], [], "Terms"],
    ["c", 7, "text"],
    ["i", 1, 7, null],
    ["e", 7, ["onLongPress"]],
    ["lp", 7, 5],
    ["t", 7, 0, 20, 100, 20, [[0, 0, "plain", 0, 0, ""]], [], [], [], null, null, [], [], "plain"],
  ]);
  player.view(6).dispatchEvent(new window.Event("pointerdown"));
  player.view(7).dispatchEvent(new window.Event("pointerdown"));
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.deepEqual(long, [7], "only the text that is not selectable");
  player.view(7).dispatchEvent(new window.Event("pointerup"));
});

test("an attachment is placed from the block, above the text", () => {
  const { player } = createTestEnv();
  player.apply([
    ["c", 9, "text"],
    ["f", 9, 20, 30, 100, 40],
    ["i", 1, 9, null],
    [
      "t",
      9,
      4,
      6,
      92,
      28,
      [[0, 5, "Made with ", 0, 0, "\uFFFC"]],
      [],
      [],
      [],
      null,
      null,
      [],
      [],
      "Made with \uFFFC",
    ],
    ["c", 10, "view"],
    ["f", 10, 12, 14, 18, 18],
    ["i", 9, 10, null],
  ]);
  const text = player.view(9);
  const box = text._text;
  const attachment = player.view(10);
  assert.equal(box.parentNode, text);
  assert.equal(attachment.parentNode, text);
  assert.ok(
    [...text.children].indexOf(attachment) > [...text.children].indexOf(box),
    "the attachment paints over the text box",
  );
  assert.equal(box.style.left, "4px");
  assert.equal(box.style.top, "6px");
  assert.equal(attachment.style.left, "12px", "the block's frame, no content origin added");
  assert.equal(attachment.style.top, "14px");
  assert.equal(attachment.style.width, "18px");
  assert.equal(attachment.style.height, "18px");
  const run = box.querySelector(".nl-run");
  assert.equal(run.style.left, "0px", "the run is inside the content box");
  assert.equal(run.style.top, "5px", "text-top is the glyph top, not the line top");
  assert.equal(run.textContent, "Made with ");
});

test("an open url command opens a new tab once", () => {
  const { player } = createTestEnv();
  const opened = [];
  player.window.open = (url, target) => {
    opened.push([url, target]);
    return null;
  };
  player.apply([["cmd", 1, "openUrl", "https://example.com/terms"]]);
  assert.deepEqual(opened, [["https://example.com/terms", "_blank"]]);
});
