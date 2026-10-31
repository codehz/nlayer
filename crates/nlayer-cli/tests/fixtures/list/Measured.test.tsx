import { expect, open, test, type App, type Frame, type Node } from "@nlayer/test";

type Hit = { node: Node; cell: Node };

/** Where a cell with no item is parked; a hidden stale row keeps its testID. */
const HIDDEN = 1_000_000;

function matches(node: Node, name: string, cell: Node | null, out: Hit[]): void {
  if (node.testID === name && cell) out.push({ node, cell });
  for (const child of node.children ?? []) matches(child, name, node, out);
}

/** The match that is showing `name`, if any. A cell the list never placed
 * has no size; its last content still names the row. */
function find(app: App, name: string): Hit | null {
  const hits: Hit[] = [];
  matches(app.tree(), name, null, hits);
  return (
    hits.find((hit) => Math.abs(hit.cell.frame.y) < HIDDEN && hit.cell.frame.height > 0) ?? null
  );
}

/** The cell that holds `name`: the list's direct child, at its natural size. */
function cell(app: App, name: string): Frame {
  const hit = find(app, name);
  if (!hit) throw new Error(`no ${name}`);
  return hit.cell.frame;
}

function listOf(app: App, name: string): Node {
  const hit = find(app, name);
  if (!hit) throw new Error(`no ${name}`);
  return hit.node;
}

/** `name`'s top inside its list's viewport. Cells sit at natural offsets. */
function top(app: App, list: string, name: string): number {
  return cell(app, name).y - (listOf(app, list).offset?.y ?? 0);
}

test("a list keeps its scroll target in place while rows measure", () => {
  const app = open("Measured.tsx");
  app.waitFor("id:single-0");
  // The 64-point header is measured, not skipped.
  expect(cell(app, "single-0").y).toBe(64);
  expect(cell(app, "single-0").height).toBe(40);

  // Estimated at 30 points each: the first pass lands short of the target.
  app.press("id:one-to-20");
  app.waitFor("id:single-22");
  // The rows above settled at their real heights; the target is at the top.
  expect(Math.abs(top(app, "single", "single-20")) <= 0.5).toBe(true);
  // The rows after it take their measured heights.
  expect(cell(app, "single-20").height).toBe(80);
  expect(cell(app, "single-21").height).toBe(40);
  expect(cell(app, "single-22").height).toBe(60);
  expect(cell(app, "single-21").y).toBe(cell(app, "single-20").y + 80);

  // The last index stops at the end of the content, where the footer is.
  app.press("id:one-to-39");
  app.waitFor("id:single-foot");
  const list = listOf(app, "single");
  const foot = cell(app, "single-foot");
  expect(foot.y).toBe((list.content?.height ?? 0) - 44);
  expect(top(app, "single", "single-foot") + foot.height).toBe(list.frame.height);
  expect(app.texts()).toContain("item 39");
  app.close();
});

test("a two-column list sizes every row to its tallest cell", () => {
  const app = open("Measured.tsx");
  app.waitFor("id:pair-6");
  // Cells are 40, 60, 80, 40, 60, 80: rows take 60, 80, 80.
  expect(cell(app, "pair-2").y - cell(app, "pair-0").y).toBe(60);
  expect(cell(app, "pair-4").y - cell(app, "pair-2").y).toBe(80);
  expect(cell(app, "pair-6").y - cell(app, "pair-4").y).toBe(80);
  // Each cell keeps its own height, in its own column.
  expect(cell(app, "pair-0").height).toBe(40);
  expect(cell(app, "pair-1").height).toBe(60);
  expect(cell(app, "pair-0").x).toBe(0);
  expect(cell(app, "pair-1").x).toBe(195);

  // The same holds for rows the list scrolled to by estimate first.
  app.press("id:two-to-20");
  expect(Math.abs(top(app, "columns", "pair-20")) <= 0.5).toBe(true);
  expect(cell(app, "pair-22").y - cell(app, "pair-20").y).toBe(80);
  expect(cell(app, "pair-26").y - cell(app, "pair-24").y).toBe(60);
  app.close();
});
