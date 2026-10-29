import { expect, open, test } from "@nlayer/test";

function byId(node, id) {
  if (node.testID === id) return node;
  for (const child of node.children) {
    const found = byId(child, id);
    if (found) return found;
  }
  return null;
}

test("the page takes a press, keys, a scroll, and the pinned inset", () => {
  const app = open("./App.tsx", {
    width: 200,
    height: 220,
    safeArea: { top: 12, right: 0, bottom: 0, left: 0 },
  });
  expect(app.texts()).toContain("Count: 0");
  app.press("id:tap");
  expect(app.texts()).toContain("Count: 1");
  app.type("Name", "ada");
  expect(app.inputs()).toEqual(["ada"]);
  app.scroll("list", 30);
  const scroll = byId(app.tree(), "list");
  expect(scroll.kind).toBe("scroll");
  expect(scroll.offset).toEqual({ x: 0, y: 30 });
  app.press("id:docs");
  expect(app.texts()).toContain("opened");
  const pad = byId(app.tree(), "pad");
  const mark = byId(app.tree(), "mark");
  expect(pad.children[0].frame.y).toBe(12);
  if (globalThis.__nlayerOs === "web") {
    expect(mark.radius).toEqual([16, 16, 16, 16]);
  } else {
    expect(globalThis.__nlayerOs === "ios" || globalThis.__nlayerOs === "android").toBe(true);
  }
});
