import { type App, type Node, expect, open, test } from "@nlayer/test";

declare global {
  var __nlayerHeadless: string | undefined;
  var __nlayerOs: string | undefined;
}

const COUNT = 48;
const HEADER = "→ Header Demo (title, subtitle, item groups)";

function deviceOnly(): void {
  const os = globalThis.__nlayerOs;
  if (globalThis.__nlayerHeadless === "1" || (os !== "android" && os !== "ios")) {
    throw new Error("run on a device: nlayer test --target android");
  }
}

function start(): App {
  deviceOnly();
  // 360pt with a horizontal safe area, so the border box and the content box
  // differ the way they did on the gallery phone.
  const app = open("App.tsx", {
    width: 360,
    height: 800,
    safeArea: { top: 0, right: 16, bottom: 0, left: 16 },
  });
  app.waitFor("id:line");
  return app;
}

function find(root: Node, id: string): Node | null {
  if (root.testID === id) return root;
  for (const child of root.children) {
    const found = find(child, id);
    if (found) return found;
  }
  return null;
}

function must(root: Node, id: string): Node {
  const found = find(root, id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

function height(app: App, id: string): number {
  return must(app.tree(), id).frame.height;
}

function assertLine(app: App, id: string, lineId: string): void {
  const h = height(app, id);
  const line = height(app, lineId);
  if (Math.abs(h - line) > 2) throw new Error(`${id} is ${h}px, one line ${lineId} is ${line}px`);
}

function assertWrapped(app: App, id: string, lineId: string): void {
  const h = height(app, id);
  const line = height(app, lineId);
  if (!(h > line * 1.6)) throw new Error(`${id} is ${h}px, one line ${lineId} is ${line}px`);
}

/** The longest "M" run that still fits the border box on one line. */
function fitWidth(app: App): number {
  const line = height(app, "line");
  let fit = 0;
  for (let n = 1; n <= COUNT; n++) {
    if (Math.abs(height(app, `u${n}`) - line) > 2) break;
    fit = n;
  }
  if (fit < 8 || fit >= COUNT) throw new Error(`no one-line edge on this device (fit=${fit})`);
  return fit;
}

test("a line that fits the border box wraps once padding or a border narrows it", () => {
  const app = start();
  const fit = fitWidth(app);
  // The first measure sees the border box (one line). The content box is
  // narrower, so the mounted view has to be tall enough for the wrap.
  assertWrapped(app, `p${fit}`, "pline");
  assertWrapped(app, `b${fit}`, "bline");
  assertWrapped(app, `a${fit}`, "aline");
  assertWrapped(app, `c${fit}`, "cline");
  assertLine(app, "p1", "pline");
  assertLine(app, "b1", "bline");
  const lines = height(app, "long") / height(app, "pline");
  if (lines < 2.4) throw new Error(`a long line grew to ${lines.toFixed(2)} rows, not three`);
  app.close();
});

test("a clamp, a fixed height, and vertical padding do not grow a line", () => {
  const app = start();
  assertLine(app, "clamp", "pline");
  const fixed = height(app, "fixed");
  if (Math.abs(fixed - 22) > 1.5) throw new Error(`fixed height is ${fixed}px`);
  const extra = height(app, "vlong") - height(app, "ulong");
  if (extra < 16 || extra > 32) throw new Error(`vertical padding added ${extra}px`);
  const short = height(app, "min-short");
  const line = height(app, "line");
  if (short < 29 || short > line * 1.6) throw new Error(`min-height short is ${short}px`);
  assertWrapped(app, "min-long", "pline");
  if (height(app, "min-long") < 30) throw new Error("min-height was dropped");
  app.close();
});

test("wrapping still holds inside a parent, a row, a fixed width, and the gallery link", () => {
  const app = start();
  assertWrapped(app, "parent-long", "parent-short");
  assertWrapped(app, "definite", "definite-short");
  assertWrapped(app, "row", "row-short");
  assertWrapped(app, "bold-long", "bold-short");
  assertLine(app, "bold-short", "pline");

  const page = must(app.tree(), "page");
  expect(page.kind).toBe("scroll");
  if (!page.content || page.content.height <= page.frame.height) {
    throw new Error("the page scroll did not grow with the lines");
  }
  if (height(app, "long") >= page.frame.height) {
    throw new Error("the line was stretched to the scroll viewport");
  }

  const header = must(app.tree(), "header-p");
  const short = must(app.tree(), "header-short");
  expect(header.text).toBe(HEADER);
  expect(short.text).toBe("→ Dialog Demo");
  if (!(header.frame.height > short.frame.height + 8)) {
    throw new Error(`header is ${header.frame.height}px, dialog link is ${short.frame.height}px`);
  }
  app.close();
});

test("shortening the line gives the extra row back, and lengthening it takes one again", () => {
  const app = start();
  assertWrapped(app, "live", "pline");
  const held = height(app, "long");
  app.press("id:shrink");
  assertLine(app, "live", "pline");
  if (Math.abs(height(app, "long") - held) > 2) {
    throw new Error("a sibling lost its row when another line shortened");
  }
  app.press("id:grow");
  assertWrapped(app, "live", "pline");
  if (Math.abs(height(app, "live") - height(app, "long")) > 2) {
    throw new Error("the restored line is not as tall as the same string");
  }
  app.close();
});
