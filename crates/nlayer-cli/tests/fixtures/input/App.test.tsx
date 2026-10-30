import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("a multiline input takes Return, reports selection and compositions", () => {
  const app = open("App.tsx");
  app.waitFor("id:field");
  app.type("id:field", "a\nb");
  expect(app.inputs()).toEqual(["a\nb"]);

  app.select("id:field", 1, 3);
  app.waitFor("text:sel:1-3");
  const field = find(app.tree(), "field");
  expect(field.selection).toEqual({ start: 1, end: 3 });
  expect(field.multiline).toBe(true);

  app.compose("id:field", "zhong", true);
  app.compose("id:field", "中", true);
  app.compose("id:field", "", false);
  const text = app.texts().join(" ");
  expect(text).toContain("compose:true:zhong");
  expect(text).toContain("compose:true:中");
  expect(text.indexOf("compose:true:中") > text.indexOf("compose:true:zhong")).toBe(true);
  expect(text.indexOf("compose:false:") > text.indexOf("compose:true:中")).toBe(true);
  app.close();
});
