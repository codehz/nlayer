import { expect, open, test, type Node } from "@nlayer/test";

function find(node: Node, name: string): Node | null {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

function rowsIn(node: Node): number {
  let count = String(node.testID ?? "").startsWith("row-") ? 1 : 0;
  for (const child of node.children ?? []) count += rowsIn(child);
  return count;
}

test("a long list renders a window, recycles cells and scrolls to an item", () => {
  const app = open("App.tsx");
  app.waitFor("id:list");
  app.waitFor("id:row-0");
  expect(find(app.tree(), "row-0") !== null).toBe(true);
  expect(find(app.tree(), "head") !== null).toBe(true);
  expect(find(app.tree(), "foot") !== null).toBe(true);
  expect(find(app.tree(), "sep") !== null).toBe(true);
  expect(find(app.tree(), "row-4999")).toBe(null);
  expect(rowsIn(app.tree()) < 100).toBe(true);

  app.scroll("id:list", 500);
  app.waitFor("id:row-10");

  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  expect(find(app.tree(), "row-4000")?.frame.height).toBe(50);
  expect(rowsIn(app.tree()) < 100).toBe(true);
  app.press("id:to-end");
  app.waitFor("text:more:1");
  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  expect(app.texts()).toContain("more:1");
  app.press("id:clear");
  app.waitFor("id:empty");
  app.close();
});
