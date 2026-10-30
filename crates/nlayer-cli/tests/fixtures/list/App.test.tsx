import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("a long list renders a window, recycles cells and scrolls to an item", () => {
  const app = open("App.tsx");
  app.waitFor("id:list");
  app.waitFor("id:row-0");
  expect(find(app.tree(), "row-0") !== null).toBe(true);
  expect(find(app.tree(), "head") !== null).toBe(true);
  expect(find(app.tree(), "foot") !== null).toBe(true);
  expect(find(app.tree(), "row-4999")).toBe(null);

  app.scroll("id:list", 500);
  app.waitFor("id:row-10");

  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  app.press("id:to-end");
  app.waitFor("text:more:1");
  app.press("id:to-4000");
  app.waitFor("id:row-4000");
  expect(find(app.tree(), "row-4000").frame.height).toBe(50);
  expect(app.texts()).toContain("more:1");
  app.close();
});
