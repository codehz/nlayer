import { expect, open, test, type App, type Frame, type Node } from "@nlayer/test";

type Hit = { node: Node; cell: Node };

/** Where a cell with no item is parked; a hidden stale row keeps its testID. */
const HIDDEN = 1_000_000;

function matches(node: Node, name: string, cell: Node | null, out: Hit[]): void {
  if (node.testID === name && cell) out.push({ node, cell });
  for (const child of node.children ?? []) matches(child, name, node, out);
}

/** The match that is showing `name`, if any. */
function find(app: App, name: string): Hit | null {
  const hits: Hit[] = [];
  matches(app.tree(), name, null, hits);
  return hits.find((hit) => Math.abs(hit.cell.frame.y) < HIDDEN) ?? null;
}

function rowsIn(node: Node): number {
  let count = String(node.testID ?? "").startsWith("row-") ? 1 : 0;
  for (const child of node.children ?? []) count += rowsIn(child);
  return count;
}

/** The text a subtree shows. */
function textIn(node: Node): string {
  if (typeof node.text === "string") return node.text;
  for (const child of node.children ?? []) {
    const text = textIn(child);
    if (text !== "") return text;
  }
  return "";
}

function rowText(app: App, name: string): string {
  const hit = find(app, name);
  return hit ? textIn(hit.node) : "";
}

/** The cell that holds `name`: the list's direct child. */
function cellFrame(app: App, name: string): Frame {
  const hit = find(app, name);
  if (!hit) throw new Error(`no ${name}`);
  return hit.cell.frame;
}

function listOf(app: App): Node {
  const hit = find(app, "list");
  if (!hit) throw new Error("no list");
  return hit.node;
}

/** `name`'s top inside the list's viewport. Cells sit at natural offsets. */
function screenTop(app: App, name: string): number {
  return cellFrame(app, name).y - (listOf(app).offset?.y ?? 0);
}

/** Whether the list shows `name` inside its viewport. */
function shown(app: App, name: string): boolean {
  const hit = find(app, name);
  if (!hit) return false;
  const frame = hit.cell.frame;
  const list = listOf(app);
  const top = frame.y - (list.offset?.y ?? 0);
  return top + frame.height > 0 && top < list.frame.height;
}

/** Whether `name`'s cell carries a separator after it. */
function separated(app: App, name: string): boolean {
  const hit = find(app, name);
  if (!hit) return false;
  return hit.cell.children.some((child) => child.testID === "sep");
}

test("a long list recycles cells across the window", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-4");
  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  // The window moved; the rows at the top lost their cells.
  expect(shown(app, "row-0")).toBe(false);
  expect(shown(app, "row-4000")).toBe(true);
  expect(rowsIn(app.tree()) < 100).toBe(true);
  // A recycled cell shows the row it holds now, not the one it had.
  expect(rowText(app, "row-4000")).toBe("row 4000");
  app.close();
});

test("a list loads more once per item count", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-0");
  // Nowhere near the end: nothing loaded yet.
  expect(app.texts()).toContain("more:0");
  app.press("id:to-end");
  app.waitFor("text:more:1");
  // Away from the end and back: the same items load once.
  app.press("id:to-top");
  app.waitFor("id:row-0");
  app.press("id:to-end");
  app.waitFor("text:more:1");
  expect(app.texts()).toContain("more:1");
  // Two more items: reaching the end again loads once more.
  app.press("id:append");
  app.waitFor("text:count:5002");
  app.waitFor("text:more:2");
  app.press("id:to-end");
  app.waitFor("text:more:2");
  expect(app.texts()).toContain("more:2");
  app.close();
});

test("a reordered list shows the item at each index, not a stale row", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-4");
  expect(rowText(app, "row-0")).toBe("row 0");
  expect(rowText(app, "row-3")).toBe("row 3");
  app.press("id:reorder");
  // Item 3 moved to the top: every row shows the item now at its index.
  expect(rowText(app, "row-0")).toBe("row 3");
  expect(rowText(app, "row-1")).toBe("row 0");
  expect(rowText(app, "row-2")).toBe("row 1");
  expect(rowText(app, "row-3")).toBe("row 2");
  expect(rowText(app, "row-4")).toBe("row 4");
  expect(app.texts()).toContain("row 3");
  app.close();
});

test("inserting items above the viewport keeps the first visible row in place", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-0");
  const before = screenTop(app, "row-0");
  const height = cellFrame(app, "row-0").height;
  expect(listOf(app).offset?.y).toBe(0);

  app.press("id:prepend");
  app.waitFor("id:row-2");
  // The row that was row 0 is row 2, on the same line of the viewport, and
  // its index accessor followed the items.
  expect(rowText(app, "row-2")).toBe("row 0");
  expect(screenTop(app, "row-2")).toBe(before);
  expect(cellFrame(app, "row-2").height).toBe(height);
  // The scroll followed the two rows above it, at their estimated height.
  expect(listOf(app).offset?.y).toBe(102);
  expect(rowText(app, "row-0")).toBe("row -2");
  expect(app.texts()).toContain("row -2");
  app.close();
});

test("emptying a list shows the placeholder, not stale rows", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-0");
  expect(shown(app, "empty")).toBe(false);

  app.press("id:shrink");
  app.waitFor("text:count:3");
  // Three items: the rows past them leave the window.
  expect(shown(app, "row-2")).toBe(true);
  expect(shown(app, "row-3")).toBe(false);
  // A separator follows every item but the last.
  expect(separated(app, "row-0")).toBe(true);
  expect(separated(app, "row-1")).toBe(true);
  expect(separated(app, "row-2")).toBe(false);

  app.press("id:clear");
  app.waitFor("id:empty");
  // Header, placeholder and footer, in that order.
  const head = cellFrame(app, "head");
  const empty = cellFrame(app, "empty");
  const foot = cellFrame(app, "foot");
  expect(head.y).toBe(0);
  expect(empty.y).toBe(head.y + head.height);
  expect(foot.y).toBe(empty.y + empty.height);
  expect(shown(app, "row-0")).toBe(false);
  expect(shown(app, "sep")).toBe(false);

  app.press("id:restore");
  app.waitFor("id:row-0");
  expect(shown(app, "empty")).toBe(false);
  expect(rowText(app, "row-0")).toBe("row 0");
  app.close();
});

test("scrolling past the end of the list stops at the last item", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-0");
  app.press("id:to-beyond");
  app.waitFor("id:row-4999");
  const list = listOf(app);
  expect(shown(app, "row-4999")).toBe(true);
  expect(shown(app, "row-0")).toBe(false);
  // The end of the content, never past it.
  expect(list.offset?.y).toBe((list.content?.height ?? 0) - list.frame.height);

  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  const at = listOf(app);
  expect(shown(app, "row-4000")).toBe(true);
  expect((at.offset?.y ?? 0) < (at.content?.height ?? 0) - at.frame.height).toBe(true);
  app.close();
});
