import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("a list opens at initialScrollIndex", () => {
  const app = open("App.tsx");
  app.waitFor("id:row-50");
  expect(find(app.tree(), "row-0")).toBe(null);
  app.close();
});
