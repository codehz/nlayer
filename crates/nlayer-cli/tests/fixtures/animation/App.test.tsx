import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("animations move values and finish promises", () => {
  const app = open("App.tsx");
  expect(find(app.tree(), "box").opacity).toBe(0);
  app.advance(150);
  const middle = find(app.tree(), "box").opacity;
  expect(middle > 0.4 && middle < 0.6).toBe(true);
  app.advance(200);
  expect(find(app.tree(), "box").opacity).toBe(1);
  expect(app.texts().join(" ")).toContain("done");
  app.advance(1000);
  const box = find(app.tree(), "box");
  expect(box.backgroundColor).toBe("#ff0000ff");
  expect(box.borderColor).toBe("#00ff00ff");
  const transform = box.transform;
  expect(transform[0].translateX).toBe(1);
  expect(transform[1].rotate).toBe(45);
  expect(transform[2].scale).toBe(2);
  expect(app.texts().join(" ")).toContain("stopped");
  app.close();
});
