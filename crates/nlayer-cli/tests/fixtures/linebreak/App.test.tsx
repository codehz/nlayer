import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("a multiline input takes Return as a line break", () => {
  const app = open("App.tsx");
  app.waitFor("id:field");
  const field = find(app.tree(), "field");
  expect(field.multiline).toBe(true);
  app.type("id:field", "a\nb");
  expect(app.inputs()).toEqual(["a\nb"]);
  expect(app.texts()).toContain("a\nb");
  app.close();
});
