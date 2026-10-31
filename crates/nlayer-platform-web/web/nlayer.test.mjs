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

test("Player screen registers event callbacks", () => {
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
  el.dispatchEvent(new window.CustomEvent("nlayer:dismiss"));
  assert.ok(dismissCalled, "Should call dismiss callback");

  el.dispatchEvent(new window.CustomEvent("nlayer:backpress"));
  assert.ok(backPressCalled, "Should call backPress callback");
});

test("Player modal registers event callbacks", () => {
  const { player, host, window } = createTestEnv();

  let showCalled = false;
  let dismissCalled = false;
  let requestCloseCalled = false;
  let closeReason = null;

  host.show = () => {
    showCalled = true;
  };
  host.dismiss = () => {
    dismissCalled = true;
  };
  host.requestClose = (id, reason) => {
    requestCloseCalled = true;
    closeReason = reason;
  };

  player.apply([["c", 2, "modal"]]);

  const el = player.view(2);

  el.dispatchEvent(new window.CustomEvent("nlayer:show"));
  assert.ok(showCalled, "Should call show callback");

  el.dispatchEvent(new window.CustomEvent("nlayer:dismiss"));
  assert.ok(dismissCalled, "Should call dismiss callback");

  el.dispatchEvent(new window.CustomEvent("nlayer:requestclose", { detail: { reason: "cancel" } }));
  assert.ok(requestCloseCalled, "Should call requestClose callback");
  assert.equal(closeReason, "cancel", "Should pass close reason");
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
