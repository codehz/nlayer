import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string, parent: any = null): any {
  if (node.testID === name) return { node, parent };
  for (const child of node.children ?? []) {
    const found = find(child, name, node);
    if (found) return found;
  }
  return null;
}

test("a list lays items out in columns", () => {
  const app = open("App.tsx");
  app.waitFor("id:cell-1");
  const first = find(app.tree(), "cell-0").parent.frame;
  const second = find(app.tree(), "cell-1").parent.frame;
  const third = find(app.tree(), "cell-2").parent.frame;
  expect(second.y).toBe(first.y);
  expect(second.x > first.x).toBe(true);
  expect(second.width).toBe(200);
  expect(third.y).toBe(first.y + 40);
  app.close();
});
