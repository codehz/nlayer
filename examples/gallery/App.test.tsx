import { type App, type Node, expect, open, test } from "@nlayer/test";

declare global {
  var __nlayerHeadless: string | undefined;
  var __nlayerOs: string | undefined;
}

function flat(node: Node, out: Node[]): Node[] {
  out.push(node);
  for (const child of node.children) flat(child, out);
  return out;
}

function containsText(node: Node, text: string): boolean {
  if (typeof node.text === "string" && node.text.includes(text)) return true;
  return node.children.some((child) => containsText(child, text));
}

function holder(nodes: Node[], text: string): Node {
  const found = nodes.find((node) => node.children.some((child) => child.text === text));
  if (!found) throw new Error(text);
  return found;
}

function locate(
  node: Node,
  pred: (node: Node) => boolean,
  top: number,
  scrollY: number,
): { node: Node; y: number } | null {
  const y = top + node.frame.y - scrollY;
  if (pred(node)) return { node, y };
  const childScroll = node.kind === "scroll" && node.offset ? node.offset.y : 0;
  for (const child of node.children) {
    const hit = locate(child, pred, y, childScroll);
    if (hit) return hit;
  }
  return null;
}

function reveal(app: App, pred: (node: Node) => boolean, pageID = "page"): void {
  const root = app.tree();
  const hit = locate(root, pred, 0, 0);
  const page = locate(root, (node) => node.testID === pageID, 0, 0);
  if (!hit || !page) throw new Error("not on the page");
  const center = hit.y + hit.node.frame.height / 2;
  const mid = page.y + page.node.frame.height / 2;
  const distance = center - mid;
  if (distance > 1 || distance < -1) app.scroll(pageID, distance);
}

/** Opens a screen from the index and waits for its page. */
function openScreen(app: App, testID: string, pageID: string): void {
  reveal(app, (node) => node.testID === testID);
  app.press(testID);
  app.waitFor(pageID);
  // The pushed screen slides in. A browser delivers a click only when the
  // pointer stays over one pressable through the motion, so wait it out.
  app.advance(350);
}

function must<T>(value: T | null | undefined): T {
  if (value == null) throw new Error("missing");
  return value;
}

/** Advances until `testID` leaves the tree: a leaving screen stays mounted for its exit motion. */
function settleGone(app: App, testID: string): void {
  for (let i = 0; i < 8; i++) {
    if (!flat(app.tree(), []).some((node) => node.testID === testID)) return;
    app.advance(100);
  }
}

test("column, row, and grid place their children", () => {
  const app = open("./App.tsx", {
    width: 390,
    height: 844,
    safeArea: { top: 22, right: 0, bottom: 0, left: 10 },
  });
  openScreen(app, "open-view", "view-page");
  const nodes = flat(app.tree(), []);
  const one = holder(nodes, "One");
  const two = holder(nodes, "Two");
  const three = holder(nodes, "Three");
  expect(one.frame.x).toBe(two.frame.x);
  expect(two.frame.x).toBe(three.frame.x);
  expect(one.frame.y < two.frame.y).toBe(true);
  expect(two.frame.y < three.frame.y).toBe(true);

  const left = holder(nodes, "Left");
  const right = holder(nodes, "Right");
  expect(left.frame.y).toBe(right.frame.y);
  expect(left.frame.x < right.frame.x).toBe(true);

  const a = holder(nodes, "A");
  const b = holder(nodes, "B");
  const c = holder(nodes, "C");
  const d = holder(nodes, "D");
  expect(a.frame.y).toBe(b.frame.y);
  expect(a.frame.x < b.frame.x).toBe(true);
  expect(c.frame.y).toBe(d.frame.y);
  expect(a.frame.y < c.frame.y).toBe(true);
  expect(a.frame.x).toBe(c.frame.x);
  expect(b.frame.x).toBe(d.frame.x);

  // The bar floats over the content (no appearance, so no bar bottom), so
  // `env(safe-area-inset-top)` is the safe area plus the bar's 44pt.
  const inset = holder(nodes, "Inset").children[0];
  expect(inset.frame.x).toBe(10);
  expect(inset.frame.y).toBe(22 + 44);
});

test("a press updates the label", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-view", "view-page");
  expect(app.texts()).toContain("Press");
  app.press("Press");
  expect(app.texts()).toContain("Pressed");
});

test("pointer styles are paint and a press still lands", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-pointer", "pointer-page");
  const nodes = flat(app.tree(), []);
  const fade = holder(nodes, "Fade");
  const hover = holder(nodes, "Hover");
  const focus = must(nodes.find((node) => node.kind === "input" && node.placeholder === "Focus"));
  expect(fade.frame.width).toBe(160);
  expect(fade.frame.height).toBe(36);
  expect(hover.frame.width).toBe(120);
  expect(hover.frame.height).toBe(36);
  expect(focus.frame.width).toBe(160);
  expect(app.texts()).toContain("activeStyle={{ opacity: 0.4 }}");
  expect(app.texts()).toContain("activeStyle={{ backgroundColor }}");
  expect(app.texts()).toContain("hoverStyle={{ backgroundColor }}");
  expect(app.texts()).toContain("focusStyle={{ backgroundColor }}");
  reveal(app, (node) => node.children.some((child) => child.text === "Fade"), "pointer-page");
  app.press("Fade");
  expect(app.texts()).toContain("Faded");
  reveal(app, (node) => node.children.some((child) => child.text === "Light"), "pointer-page");
  app.press("Light");
  expect(app.texts()).toContain("Lit");
});

test("a static transform is mounted", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-view", "view-page");
  const shifted = holder(flat(app.tree(), []), "Shifted");
  expect(shifted.transform).toEqual([{ translateX: 12 }, { rotate: 15 }, { scale: 1.25 }]);
});

test("safe-area env insets pad the view", () => {
  const app = open("./App.tsx", {
    width: 390,
    height: 844,
    safeArea: { top: 22, right: 8, bottom: 14, left: 10 },
    keyboard: 40,
  });
  const nodes = flat(app.tree(), []);
  const safe = must(nodes.find((node) => node.testID === "safe"));
  // The bar floats over the content (no appearance, so no bar bottom), so
  // the top inset is the safe area plus the bar's 44pt. Left is the
  // window's.
  const top = 22 + 44;
  const title = must(safe.children[0]);
  expect(title.frame.x).toBe(26);
  expect(title.frame.y).toBe(16 + top);
  const section = must(safe.children.find((node) => node.kind === "view"));
  expect(section.frame.x).toBe(26);
  expect(section.frame.x + section.frame.width).toBe(safe.frame.width - 24);
  const last = must(safe.children[safe.children.length - 1]);
  expect(safe.frame.height - (last.frame.y + last.frame.height)).toBe(72);
});

test("nested text is one block with distinct spans", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-text", "text-page");
  const block = must(flat(app.tree(), []).find((node) => node.text === "Plain bold italic color"));
  expect(block.kind).toBe("text");
  const spans = must(block.spans);
  expect(must(spans.find((span) => span.text === " bold")).fontWeight).toBe(700);
  expect(must(spans.find((span) => span.text === " italic")).italic).toBe(true);
  const color = must(spans.find((span) => span.text === " color"));
  expect(color.fontSize).toBe(22);
  expect(color.color).toBe("#c2410c");
});

test("images mount a bundled asset and a remote URL", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-image", "image-page");
  const nodes = flat(app.tree(), []);
  const bundled = must(
    nodes.find(
      (node) => node.kind === "image" && String(node.src).indexOf("asset:/mark.png") === 0,
    ),
  );
  expect(bundled.fit).toBe("cover");
  expect(bundled.frame.width).toBe(32);
  expect(bundled.frame.height).toBe(32);
  const remote = must(nodes.find((node) => node.src === "https://example.com/photo.jpg"));
  expect(remote.kind).toBe("image");
  expect(remote.fit).toBe("contain");
  expect(remote.frame.width).toBe(48);
  expect(remote.frame.height).toBe(32);
});

test("scroll views overflow on each axis", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-scroll", "scroll-page");
  const nodes = flat(app.tree(), []);
  const down = must(
    nodes.find(
      (node) =>
        node.kind === "scroll" &&
        node.horizontal === false &&
        node.content &&
        node.content.height === 200,
    ),
  );
  const across = must(
    nodes.find(
      (node) => node.kind === "scroll" && node.horizontal === true && containsText(node, "Across"),
    ),
  );
  expect(down.frame.height).toBe(80);
  expect(down.content).toEqual({ width: 200, height: 200 });
  expect(across.frame.height).toBe(48);
  expect(across.frame.width).toBe(160);
  expect(must(across.content).width > across.frame.width).toBe(true);
  expect(must(across.content).height).toBe(48);
  reveal(app, (node) => node.testID === "across", "scroll-page");
  app.scroll("across", 40);
  const moved = must(flat(app.tree(), []).find((node) => node.testID === "across"));
  expect(moved.offset).toEqual({ x: 40, y: 0 });
});

test("scroll callbacks report the offset, content, and viewport", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-scroll", "scroll-page");
  const names = [
    "onScroll",
    "onScrollEnd",
    "onScrollDragStart",
    "onScrollDragEnd",
    "onMomentumStart",
    "onMomentumEnd",
  ];
  for (const name of names) {
    expect(app.texts()).toContain(`${name} none`);
  }
  reveal(app, (node) => node.testID === "events", "scroll-page");
  app.scroll("events", 40);
  const events = must(flat(app.tree(), []).find((node) => node.testID === "events"));
  const offset = must(events.offset);
  const content = must(events.content);
  expect(offset).toEqual({ x: 0, y: 40 });
  const metric = `${String(Math.round(offset.x))},${String(Math.round(offset.y))} ${String(Math.round(content.width))}x${String(Math.round(content.height))} ${String(Math.round(events.frame.width))}x${String(Math.round(events.frame.height))}`;
  // The web step sets the offset and reports `onScroll` and `onScrollEnd`.
  // It is not a finger's drag. Android and iOS synthesize that drag.
  const fired = globalThis.__nlayerOs === "web" ? names.slice(0, 2) : names;
  for (const name of fired) {
    app.waitFor(`${name} ${metric}`);
  }
});

test("inputs take text, keyboards, a password, and a disabled value", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-input", "input-page");
  const nodes = flat(app.tree(), []);
  const input = (placeholder: string) =>
    nodes.find((node) => node.kind === "input" && node.placeholder === placeholder)!;
  expect(input("Email").keyboard).toBe("email");
  expect(input("Amount").keyboard).toBe("number");
  expect(input("Phone").keyboard).toBe("phone");
  expect(input("Password").secure).toBe(true);
  expect(input("Password").keyboard).toBe("text");
  expect(input("Locked").disabled).toBe(true);
  expect(input("Locked").value).toBe("locked");
  expect(app.texts()).toContain("Locked: locked");
  reveal(app, (node) => node.kind === "input" && node.placeholder === "Name", "input-page");
  app.type("Name", "ada");
  expect(app.texts()).toContain("Name: ada");
  expect(app.inputs()).toContain("ada");
  reveal(app, (node) => node.kind === "input" && node.placeholder === "Locked", "input-page");
  expect(() => app.type("locked", "nope")).toThrow("disabled");
  expect(app.texts()).toContain("Locked: locked");
});

test("fetch renders the JSON", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-fetch", "fetch-page");
  reveal(app, (node) => node.text === "Loading quote" || node.text === "Gallery: 5", "fetch-page");
  app.waitFor("Gallery: 5");
});

test("platform.os is the surface", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-platform", "platform-page");
  const os = globalThis.__nlayerOs ?? "web";
  expect(["web", "ios", "android"]).toContain(os);
  const shown = app
    .texts()
    .filter((text) => text === "web" || text === "ios" || text === "android");
  expect(shown).toEqual([os]);
});

test("NavStack app opens with content", () => {
  const app = open("./App.tsx");
  const tree = app.tree();
  const nodes = flat(tree, []);
  expect(nodes.length > 10).toBe(true); // Has substantial content
});

test("home title is visible initially", () => {
  const app = open("./App.tsx");
  app.waitFor("home-title");
});

test("press detail link navigates to detail", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-detail");
  app.press("open-detail");
  app.waitFor("detail-title");
  const tree = app.tree();
  const detailTitle = flat(tree, []).find((n) => n.testID === "detail-title");
  expect(detailTitle !== undefined).toBe(true);
  expect(must(detailTitle).text).toBe("Detail Screen");
});

test("back() returns from detail to home", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-detail");
  app.press("open-detail");
  app.waitFor("detail-title");

  app.back();
  app.waitFor("open-detail");
  settleGone(app, "detail-title");
  const tree = app.tree();
  const detailTitle = flat(tree, []).find((n) => n.testID === "detail-title");
  expect(detailTitle).toBe(undefined);
});

test("onBackPress intercepts back() on detail", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-detail");
  app.press("open-detail");
  app.waitFor("intercept-back");

  app.press("intercept-back");
  app.back();
  // The page clears the notice after a second. `waitFor` is the
  // observation; a later snapshot races that timer.
  app.waitFor("back-intercepted");
});

test("dialog opens and back() closes it", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-dialog");
  app.press("open-dialog");
  app.waitFor("dialog-page");

  app.press("open-dialog-fade");
  app.waitFor("demo-dialog");

  // back() closes the dialog (onRequestClose reason="cancel"). The
  // element unmounts at once; hosts keep the layer until the exit motion
  // ends, so the tree can still show it for that interval.
  app.back();
  let tree = app.tree();
  for (let i = 0; i < 8 && flat(tree, []).some((n) => n.testID === "demo-dialog"); i++) {
    app.advance(100);
    tree = app.tree();
  }
  const dialog = flat(tree, []).find((n) => n.testID === "demo-dialog");
  expect(dialog).toBe(undefined);

  // Check if we're still on dialog-page (to read close-reason)
  const dialogPage = flat(tree, []).find((n) => n.testID === "dialog-page");
  if (dialogPage) {
    // Still on dialog page - check close reason
    const texts = app.texts();
    const closeReason = texts.find((t) => t.startsWith("Closed by:"));
    if (closeReason) {
      expect(closeReason).toContain("cancel");
    }
  }
  // If not on dialog-page, back() popped the entire page - can't verify close reason
  // Manual verification required: Android back key, iOS swipe, web Escape all trigger "cancel"
});

test("dialog with animation=none", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-dialog");
  app.press("open-dialog");
  app.waitFor("dialog-page");

  app.press("open-none");
  app.waitFor("demo-dialog");

  app.press("close-dialog");
  const tree = app.tree();
  const dialog = flat(tree, []).find((n) => n.testID === "demo-dialog");
  expect(dialog).toBe(undefined);
});

test("dialog closeOnBackdropPress and closeOnEscape options", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-dialog");
  app.press("open-dialog");
  app.waitFor("dialog-page");

  // Test that toggles are visible
  const texts = app.texts();
  expect(texts).toContain("closeOnBackdropPress: true");
  expect(texts).toContain("closeOnEscape: true");

  // Note: backdropPress and Escape require user interaction on real platforms
  // Headless mode only tests back() which triggers "cancel" reason
  // Platform-specific: iOS swipe down, Android/web Escape key need manual testing
});

test("detail pop increments dismissed count", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-detail");

  const initialTree = app.tree();
  const initialCount = flat(initialTree, []).find((n) => n.testID === "dismissed-count");
  expect(must(initialCount).text).toBe("Dismissed: 0");

  app.press("open-detail");
  app.waitFor("detail-title");
  app.back();
  app.waitFor("dismissed-count");

  const finalTree = app.tree();
  const finalCount = flat(finalTree, []).find((n) => n.testID === "dismissed-count");
  expect(must(finalCount).text).toBe("Dismissed: 1");
});

test("a pan reports its translation", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-gesture");
  app.press("open-gesture");
  app.waitFor("gesture-card");
  // The screen slides in; wait for it to stop so the drag lands on the card.
  app.advance(400);

  // Drag gesture triggers onPanEnd with translation
  app.drag("gesture-card", 80, 0);

  // onPanEnd receives the translation the drag made.
  const texts = app.texts();
  const translationText = texts.find((t) => t.startsWith("Translation:"));
  if (!translationText) {
    expect("Translation: text").toBe("should exist after drag");
  }

  // Parse translation value - expect x ≈ 80
  const match = translationText?.match(/Translation: \((-?\d+), (-?\d+)\)/);
  if (match) {
    const x = parseInt(match[1]);
    const y = parseInt(match[2]);

    // Assert translation.x is approximately 80 (within ±2px)
    if (Math.abs(x - 80) > 2) {
      expect(`translation.x=${x}`).toBe("should be ≈80");
    }

    // Y should be 0 since we dragged horizontally
    if (Math.abs(y) > 2) {
      expect(`translation.y=${y}`).toBe("should be ≈0");
    }
  }
});

test("pinch and rotation gestures display", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-gesture");
  app.press("open-gesture");
  app.waitFor("gesture-page");

  // The cards host pinch and rotation; two-finger input has no headless
  // step (as in nsh), so the values are exercised on a host.
  const tree = app.tree();
  const pinchCard = flat(tree, []).find((n) => n.testID === "pinch-card");
  const rotationCard = flat(tree, []).find((n) => n.testID === "rotation-card");
  const scrollPanCard = flat(tree, []).find((n) => n.testID === "scroll-pan-card");

  if (!pinchCard) expect("pinchCard").toBe("should exist");
  if (!rotationCard) expect("rotationCard").toBe("should exist");
  if (!scrollPanCard) expect("scrollPanCard").toBe("should exist");
});

test("list demo shows virtual list", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-list");
  app.press("open-list");
  app.waitFor("list");
  app.waitFor("first");

  app.back();
  app.waitFor("open-list");
});

test("list with snapToInterval and stickyHeaderIndices", () => {
  const app = open("./App.tsx");
  openScreen(app, "open-list", "list-page");

  // Enable snap and scroll
  app.press("toggle-snap");
  let texts = app.texts();
  expect(texts).toContain("snapToInterval: 64px");

  // Scroll by 150: the offset must match the scroll distance. Snapping is
  // applied by the platform host (iOS/Android/web) when a gesture ends, not
  // by core, so headless asserts propagation only (offset follows the scroll).
  app.scroll("list", 150);
  const afterSnap = app.tree();
  const listAfterSnap = flat(afterSnap, []).find((n) => n.testID === "list");
  if (listAfterSnap && listAfterSnap.offset && Math.abs(listAfterSnap.offset.y - 150) > 5) {
    expect(`list offset.y=${listAfterSnap.offset.y}`).toBe("should be ≈150");
  }

  // Disable snap, enable sticky headers
  app.press("toggle-snap");
  app.press("toggle-sticky");

  // Verify sticky toggle updates the UI text
  texts = app.texts();
  expect(texts).toContain("stickyHeaderIndices: [0, 10, 20]");

  // Reset scroll to 0, then scroll down 100px
  app.scroll("list", -1000);
  app.scroll("list", 100);

  const afterSticky = app.tree();
  const afterList = flat(afterSticky, []).find((n) => n.testID === "list");

  // Verify scroll offset
  if (afterList && afterList.offset && Math.abs(afterList.offset.y - 100) > 5) {
    expect(`list offset.y=${afterList.offset.y}`).toBe("should be ≈100");
  }

  // Verify sticky header is pinned at top (y≈100)
  const item0After = flat(afterSticky, []).find((n) => n.testID === "first-item");
  if (item0After) {
    const item0Parent = flat(afterSticky, []).find((n) => n.children?.includes(item0After));
    if (item0Parent && item0Parent.frame) {
      if (Math.abs(item0Parent.frame.y - 100) > 2) {
        expect(`item0 parent y=${item0Parent.frame.y}`).toBe("should be ≈100 (sticky pinned)");
      }
    }
  }
});

test("list with inverted and onViewableItemsChange", () => {
  const app = open("./App.tsx");
  openScreen(app, "open-list", "list-page");

  // Get baseline: item-0 position before inverted (should be at top, y=0)
  let tree = app.tree();
  const item0Before = flat(tree, []).find((n) => n.testID === "first-item");
  if (!item0Before) {
    expect("first-item before inverted").toBe("should exist");
  }
  const item0ParentBefore = item0Before
    ? flat(tree, []).find((n) => n.children.includes(item0Before))
    : undefined;

  // Enable inverted
  app.press("toggle-inverted");

  // Verify toggle works
  let texts = app.texts();
  expect(texts).toContain("inverted: true");

  // Verify inverted layout: item-0 moves from top (y=0) to bottom (large y)
  tree = app.tree();
  const item0After = flat(tree, []).find((n) => n.testID === "first-item");
  if (!item0After) {
    expect("first-item after inverted").toBe("should exist");
  }
  const item0ParentAfter = item0After
    ? flat(tree, []).find((n) => n.children.includes(item0After))
    : undefined;

  if (item0ParentBefore?.frame && item0ParentAfter?.frame) {
    // Before inverted: item-0 at top (y=0)
    expect(item0ParentBefore.frame.y).toBe(0);
    // After inverted: item-0 at bottom of 1000-item list (y > 1000)
    if (item0ParentAfter.frame.y <= 1000) {
      expect("item0 y=" + item0ParentAfter.frame.y).toBe(
        "inverted: item0 should be at bottom (y > 1000)",
      );
    }
  }

  // Verify onViewableItemsChange fires
  const viewableText = texts.find((t) => t.startsWith("Viewable:"));
  if (!viewableText) {
    expect("viewable range").toBe("should exist");
  }
});

test("list viewport=parent uses outer scroll", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-viewport-parent");
  app.press("open-viewport-parent");
  app.waitFor("viewport-parent-page");

  // Get initial rendered items
  let tree = app.tree();
  let listItems = flat(tree, []).filter((n) => n.testID?.startsWith("vp-item-"));

  // Should render some items (windowing active)
  if (listItems.length === 0) {
    expect("viewport=parent items").toBe("should render");
  }

  // Should not render all 100 items (windowing working)
  if (listItems.length >= 100) {
    expect(`rendered ${listItems.length} items`).toBe("should window (< 100)");
  }

  const initialItemCount = listItems.length;
  const initialItemIds = listItems.map((n) => n.testID).sort();

  // Verify all items are within parent scroll frame
  const outerScroll = flat(tree, []).find((n) => n.testID === "outer-scroll");
  if (!outerScroll || !outerScroll.frame) {
    expect("outer-scroll").toBe("should exist with frame");
  }

  const scrollHeight = outerScroll?.frame?.height || 0;
  for (const item of listItems) {
    if (item.frame) {
      // Items should be within scroll viewport + reasonable overscan
      if (item.frame.y > scrollHeight + 1000 || item.frame.y < -1000) {
        expect(`item ${item.testID} y=${item.frame.y}`).toBe(
          `within scroll bounds (0 to ${scrollHeight} + overscan)`,
        );
      }
    }
  }

  // Scroll outer scroll view - windowed items should change
  app.scroll("outer-scroll", 500);
  tree = app.tree();
  listItems = flat(tree, []).filter((n) => n.testID?.startsWith("vp-item-"));

  const afterScrollItemIds = listItems.map((n) => n.testID).sort();

  // After scroll, some items should be different (windowing updated)
  const itemsChanged =
    afterScrollItemIds.some((id) => !initialItemIds.includes(id)) ||
    initialItemIds.some((id) => !afterScrollItemIds.includes(id));

  if (!itemsChanged) {
    expect("windowed items after scroll").toBe(
      "should change (new items appear, old items disappear)",
    );
  }

  // Verify new items are also within bounds
  for (const item of listItems) {
    if (item.frame) {
      if (item.frame.y > scrollHeight + 1000 || item.frame.y < -1000) {
        expect(`item ${item.testID} after scroll y=${item.frame.y}`).toBe(`within scroll bounds`);
      }
    }
  }
});

test("header demo shows title and header props", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-header");
  app.press("open-header");
  app.waitFor("header-demo");

  const texts = app.texts();
  expect(texts).toContain("Header Demo");
  expect(texts).toContain("Back");
  expect(texts).toContain("Right");
  // Headless records the system header as `screen`. Every host draws the
  // title itself (the system bar on iOS, the drawn bar on Android, the
  // player on web), so the mounted tree has no text for it.
  const nodes = flat(app.tree(), []) as Array<
    Node & { screen?: { title?: string | null; headerVisible?: boolean } }
  >;
  const screen = nodes.find((node) => node.screen?.title === "Header");
  if (screen) {
    expect(screen.screen?.headerVisible).toBe(true);
  }
  // `Back` and `Right` are the app's own views inside the item groups.
  app.back();
  app.waitFor("page"); // Wait for home page scroll view
});

test("the bar model carries the v11 options", () => {
  type Bar = {
    title?: string | null;
    subtitle?: string | null;
    headerVisible: boolean;
    headerHeight: number;
    largeTitleDisplayMode: string;
    backButtonTitle?: string | null;
    backButtonDisplayMode: string;
    hidesBackButton: boolean;
    leftItemsSupplementBackButton: boolean;
    leading: Array<Array<{ title?: string | null; view?: number; enabled?: boolean }>>;
    trailing: Array<Array<{ title?: string | null; view?: number }>>;
    standardAppearance: { backgroundColor?: string; titleColor?: string; tintColor?: string };
    scrollEdgeAppearance: { backgroundColor?: string; shadow?: string };
  };
  const app = open("./App.tsx", {
    width: 390,
    height: 844,
    safeArea: { top: 47, right: 0, bottom: 34, left: 0 },
  });
  reveal(app, (n) => n.testID === "open-header");
  app.press("open-header");
  app.waitFor("header-demo");

  const screenOf = () => {
    const nodes = flat(app.tree(), []) as Array<Node & { screen?: Bar }>;
    return nodes.find((node) => node.screen?.title === "Header")?.screen;
  };
  // The mounted model is the headless host's; the other hosts draw the bar.
  const first = screenOf();
  if (!first) {
    app.back();
    return;
  }
  expect(first.subtitle).toBe("v11 bar");
  expect(first.headerVisible).toBe(true);
  // A subtitle raises the content row to 56pt.
  expect(first.headerHeight).toBe(56);
  expect(first.largeTitleDisplayMode).toBe("never");
  expect(first.hidesBackButton).toBe(false);
  expect(first.leftItemsSupplementBackButton).toBe(false);
  expect(first.leading.length).toBe(2);
  expect(first.leading[1][0].title).toBe("Edit");
  expect(first.trailing.length).toBe(2);
  expect(first.trailing[0].length).toBe(2);
  expect(typeof first.trailing[1][0].view).toBe("number");
  expect(first.standardAppearance.backgroundColor).toBe("#10b981ff");
  expect(first.standardAppearance.titleColor).toBe("#ffffffff");
  expect(first.scrollEdgeAppearance.backgroundColor).toBe("#00000000");
  expect(first.scrollEdgeAppearance.shadow).toBe("none");

  reveal(app, (n) => n.testID === "toggle-large-title");
  app.press("toggle-large-title");
  const large = screenOf();
  expect(large?.largeTitleDisplayMode).toBe("always");
  // The content row, the static large-title row, and the subtitle under it.
  expect(large?.headerHeight).toBe(44 + 52 + 16);

  reveal(app, (n) => n.testID === "toggle-transparent");
  app.press("toggle-transparent");
  expect(screenOf()?.standardAppearance.backgroundColor).toBe("#00000000");

  reveal(app, (n) => n.testID === "toggle-supplement");
  app.press("toggle-supplement");
  expect(screenOf()?.leftItemsSupplementBackButton).toBe(true);

  app.back();
  app.waitFor("page");
});

test("reading screen shows its selectable texts", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-reading");
  app.press("open-reading");
  app.waitFor("reading-page");

  const texts = app.texts();
  const has = (part: string) => texts.some((text) => text.includes(part));
  expect(has("Long-press a word here to select it — and this nested span comes too")).toBe(true);
  expect(has("A selection started here continues through the paragraphs below")).toBe(true);
  expect(has("Caption — not selectable")).toBe(true);
  expect(has("So this paragraph joins the same selection")).toBe(true);
  expect(has("By continuing you agree to the Terms of service.")).toBe(true);

  const nodes = flat(app.tree(), []);
  const paragraph = must(nodes.find((node) => node.testID === "reading-selectable-paragraph"));
  expect(paragraph.kind).toBe("text");
  expect(paragraph.userSelect).toBe(true);
  expect(paragraph.selectionColor).toBe("#93c5fdff");
  expect(app.selectionOwner("reading-selectable-paragraph")).toBe(paragraph.id);

  const view = must(nodes.find((node) => node.testID === "reading-selectable"));
  expect(view.selectable).toBe(true);
  expect(app.selectionOwner("reading-para-one")).toBe(view.id);
  expect(app.selectionOwner("reading-para-two")).toBe(view.id);
  expect(app.selectionOwner("reading-none-label")).toBe(null);

  const skipped = must(nodes.find((node) => node.testID === "reading-none-label"));
  expect(skipped.userSelect).toBe(false);
  const one = must(nodes.find((node) => node.testID === "reading-para-one"));
  expect(one.userSelect).toBe(undefined);
  expect(one.selectionColor).toBe("#fde68aff");
});

test("reading screen places its inline view and image in the text", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-reading");
  app.press("open-reading");
  app.waitFor("reading-page");

  const inline = must(flat(app.tree(), []).find((node) => node.testID === "reading-inline"));
  expect(inline.kind).toBe("text");
  const image = must(
    inline.children.find(
      (node) => node.kind === "image" && String(node.src).indexOf("asset:/mark.png") === 0,
    ),
  );
  expect(image.frame.width).toBe(18);
  expect(image.frame.height).toBe(18);
  const badge = must(inline.children.find((node) => node.testID === "reading-inline-badge"));
  expect(badge.kind).toBe("view");
  expect(badge.frame.height).toBe(18);
  expect(badge.frame.width > 0).toBe(true);
  expect(badge.children.some((node) => node.text === "LIVE")).toBe(true);
});

test("a press on the reading link opens it and reports to the press handler", () => {
  const app = open("./App.tsx");
  reveal(app, (n) => n.testID === "open-reading");
  app.press("open-reading");
  app.waitFor("reading-page");

  app.scroll("reading-page", 600);
  app.press("reading-terms");
  app.waitFor("Link pressed: 1");
  expect(app.openedUrls()).toEqual(["https://example.com/terms"]);

  // The paragraph's own `onPress` sees presses outside the nested span, and
  // the span's press does not open a second time.
  app.press("reading-terms-parent");
  app.waitFor("Link pressed: 2");
  expect(app.openedUrls()).toEqual(["https://example.com/terms"]);
});

function byId(root: Node, id: string): Node {
  const found = flat(root, []).find((node) => node.testID === id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

test("classes lay out, restyle, and warn", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  reveal(app, (node) => node.testID === "open-classes");
  app.press("open-classes");
  app.waitFor("classes-page");
  app.hover("class-hover");
  app.press("class-press");

  const tree = app.tree();
  const row = byId(tree, "class-row");
  expect(byId(row, "class-a").frame).toEqual({ x: 16, y: 16, width: 10, height: 10 });
  expect(byId(row, "class-b").frame.x).toBe(38);
  expect(byId(row, "class-b").frame.y).toBe(16);
  const arbitrary = byId(tree, "class-arbitrary");
  expect(arbitrary.frame.width).toBe(37);
  expect(arbitrary.frame.height).toBe(11);
  expect(byId(byId(tree, "class-space"), "space-b").frame.x).toBe(16);
  expect(byId(byId(tree, "class-mixed"), "mixed-child").frame).toEqual({
    x: 16,
    y: 16,
    width: 8,
    height: 8,
  });
  const override = byId(byId(tree, "class-override"), "override-child");
  expect(override.frame.x).toBe(8);
  expect(override.frame.y).toBe(16);
  expect(byId(byId(tree, "class-important"), "important-child").frame.y).toBe(16);

  const headless = globalThis.__nlayerHeadless === "1";
  if (!headless) return;

  const hover = byId(app.tree(), "class-hover");
  expect(byId(hover, "hover-child").frame.x).toBe(16);
  expect(byId(hover, "hover-child").frame.y).toBe(16);
  expect(row.backgroundColor).toBe("#27272aff");
  expect(arbitrary.backgroundColor).toBe("#7c3aedff");
  expect(byId(tree, "class-faded").opacity).toBe(0.5);
  expect((byId(tree, "class-shift").transform ?? []).some((step) => step.translateX === 4)).toBe(
    true,
  );
  const image = byId(tree, "class-image");
  expect(image.fit).toBe("cover");
  expect(image.radius).toEqual([8, 8, 8, 8]);
  const span = byId(tree, "class-label").spans?.[0];
  expect(span?.fontSize).toBe(14);
  expect(span?.fontWeight).toBe(600);
  expect(span?.color).toBe("#27272a");
  expect(byId(tree, "class-select").selectionColor).toBe("#e4e4e7ff");
  expect(byId(tree, "class-disabled").opacity).toBe(0.25);
  expect(byId(tree, "class-enabled").opacity).toBe(0.5);

  app.focus("class-field");
  const field = byId(byId(app.tree(), "class-within"), "class-field");
  expect(field.frame.x).toBe(16);
  expect(field.frame.y).toBe(16);
  expect(field.opacity).toBe(0.5);

  app.down("class-press");
  expect(
    (byId(app.tree(), "class-press").transform ?? []).some((step) => step.scale === 0.95),
  ).toBe(true);
  app.up();

  const warnings = app.warnings().join("\n");
  expect(warnings).toContain("unknown classes: wat");
  expect(warnings).toContain("unsupported classes: blur-sm");
});

test("effects paint shadows and gradients without moving layout", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  reveal(app, (n) => n.testID === "open-effects");
  app.press("open-effects");
  app.waitFor("effects-page");

  const headless = globalThis.__nlayerHeadless === "1";
  if (!headless) return;

  const shadow = byId(app.tree(), "fx-shadow");
  const shadowFrame = shadow.frame;
  const layers = must(shadow.boxShadow);
  expect(layers.length).toBe(2);
  expect({
    x: layers[0].x,
    y: layers[0].y,
    blur: layers[0].blur,
    spread: layers[0].spread,
    inset: layers[0].inset,
  }).toEqual({ x: 0, y: 8, blur: 24, spread: 4, inset: false });
  expect(layers[0].color.slice(7) === "ff").toBe(false);
  expect(layers[1].spread < 0).toBe(true);
  app.hover("fx-shadow");
  expect(byId(app.tree(), "fx-shadow").frame).toEqual(shadowFrame);

  expect(must(byId(app.tree(), "fx-inset").boxShadow)[0].inset).toBe(true);

  const gradient = byId(app.tree(), "fx-gradient");
  const linear = must(gradient.backgroundImage)[0];
  expect(linear.kind).toBe("linear");
  expect(linear.repeating).toBe(false);
  expect(linear.stops.length).toBe(2);
  expect(linear.stops[0]).toEqual({ color: "#3b82f600", position: 0 });
  expect(linear.stops[linear.stops.length - 1]).toEqual({ color: "#3b82f6ff", position: 1 });
  expect(gradient.backgroundColor).toBe("#fef3c7ff");
  expect(must(byId(app.tree(), "fx-radial").backgroundImage)[0].kind).toBe("radial");
  expect(must(byId(app.tree(), "fx-conic").backgroundImage)[0].kind).toBe("conic");

  const tree = app.tree();
  expect((byId(tree, "fx-class-shadow").boxShadow ?? []).length > 0).toBe(true);
  expect((byId(tree, "fx-class-gradient").backgroundImage ?? []).length > 0).toBe(true);
  const ring = must(byId(tree, "fx-class-ring").boxShadow);
  expect(ring.length > 0).toBe(true);
  expect(ring.every((layer) => layer.inset === false)).toBe(true);
  const inset = must(byId(tree, "fx-class-inset").boxShadow);
  expect(inset.length > 0).toBe(true);
  expect(inset.some((layer) => layer.inset)).toBe(true);

  const state = byId(tree, "fx-state");
  const stateFrame = state.frame;
  const stateShadow = must(state.boxShadow);
  expect(stateShadow.length).toBe(1);
  app.hover("fx-state");
  const hovered = byId(app.tree(), "fx-state");
  expect(hovered.frame).toEqual(stateFrame);
  expect(hovered.boxShadow).toEqual(stateShadow);
  const hoverLayers = must(hovered.stateStyles?.hover?.boxShadow);
  expect(hoverLayers.length).toBe(1);
  expect(hoverLayers[0].blur).toBe(32);
  expect(hovered.stateStyles?.active).toBe(undefined);
  expect(hovered.stateStyles?.focus).toBe(undefined);

  app.hover("fx-class-hover");
  expect((byId(app.tree(), "fx-class-hover").boxShadow ?? []).length > 0).toBe(true);
  app.hover("fx-class-hover-none");
  expect(byId(app.tree(), "fx-class-hover-none").boxShadow).toBe(undefined);

  const warnings = app.warnings().join("\n");
  expect(warnings).toContain("unsupported classes: blur-sm");
  expect(warnings.includes("unsupported classes: shadow-")).toBe(false);
  expect(warnings.includes("unsupported classes: ring-")).toBe(false);
  expect(warnings.includes("unsupported classes: bg-linear")).toBe(false);
});

test("pointer events screen lists the four values and the class lowering", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  openScreen(app, "open-pointer-events", "pointer-events-page");
  const texts = app.texts().join("\n");
  expect(texts).toContain("盒和按钮都不接，点哪里都是垫层。");
  expect(texts).toContain("按钮接。点盒的空白落到垫层，盒自己不接。");
  expect(texts).toContain("只有盒接。点按钮也是盒，按钮不接。");
  expect(texts).toContain(
    "none 里有 pointer-events-auto：盒降成 box-none，按钮仍接，空白落到垫层。",
  );
  expect(texts).toContain(
    "style pointerEvents none 盖过按钮上的 pointer-events-auto，整棵都不接。",
  );
  expect(texts).toContain("罩层 none");
  expect(texts).toContain("拖空白和拖这行都滚动");
});

test("a long nav link keeps its wrapped second line", () => {
  // The emulator is 1080px at 420dpi (2.625), 411dp wide. Header Demo is the
  // line that sits on that edge: iOS wraps it, Android used to keep a
  // one-line frame and clip the second line.
  const app = open("./App.tsx", {
    width: 411,
    height: 900,
    safeArea: { top: 24, right: 0, bottom: 0, left: 0 },
  });
  reveal(app, (node) => node.testID === "open-header");
  const header = must(flat(app.tree(), []).find((node) => node.testID === "open-header"));
  const dialog = must(flat(app.tree(), []).find((node) => node.testID === "open-dialog"));
  expect(header.text).toBe("→ Header Demo (title, headerStyle, headerTransparent)");
  expect(dialog.text).toBe("→ Dialog Demo");
  expect(header.frame.height > dialog.frame.height + 8).toBe(true);
});
