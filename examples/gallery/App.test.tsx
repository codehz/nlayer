import { expect, open, test } from "@nlayer/test";

function flat(node, out) {
  out.push(node);
  for (const child of node.children) flat(child, out);
  return out;
}

function containsText(node, text) {
  if (typeof node.text === "string" && node.text.includes(text)) return true;
  return node.children.some((child) => containsText(child, text));
}

function holder(nodes, text) {
  return nodes.find((node) => node.children.some((child) => child.text === text));
}

function locate(node, pred, top, scrollY) {
  const y = top + node.frame.y - scrollY;
  if (pred(node)) return { node, y };
  const childScroll = node.kind === "scroll" && node.offset ? node.offset.y : 0;
  for (const child of node.children) {
    const hit = locate(child, pred, y, childScroll);
    if (hit) return hit;
  }
  return null;
}

function reveal(app, pred) {
  const root = app.tree();
  const hit = locate(root, pred, 0, 0);
  const page = locate(root, (node) => node.testID === "page", 0, 0);
  const center = hit.y + hit.node.frame.height / 2;
  const mid = page.y + page.node.frame.height / 2;
  const distance = center - mid;
  if (distance > 1 || distance < -1) app.scroll("page", distance);
}

test("column, row, and grid place their children", () => {
  const app = open("./App.tsx", { width: 390, height: 844, safeArea: { top: 22, right: 0, bottom: 0, left: 10 } });
  const nodes = flat(app.tree(), []);
  const one = holder(nodes, "One");
  const two = holder(nodes, "Two");
  const three = holder(nodes, "Three");
  expect(one.frame.x).toBe(two.frame.x);
  expect(two.frame.x).toBe(three.frame.x);
  expect(one.frame.y < two.frame.y).toBe(true);
  expect(two.frame.y < three.frame.y).toBe(true);

  const left = holder(nodes, "Left");
  const right = holder(nodes, "Right");
  expect(left.frame.y).toBe(right.frame.y);
  expect(left.frame.x < right.frame.x).toBe(true);

  const a = holder(nodes, "A");
  const b = holder(nodes, "B");
  const c = holder(nodes, "C");
  const d = holder(nodes, "D");
  expect(a.frame.y).toBe(b.frame.y);
  expect(a.frame.x < b.frame.x).toBe(true);
  expect(c.frame.y).toBe(d.frame.y);
  expect(a.frame.y < c.frame.y).toBe(true);
  expect(a.frame.x).toBe(c.frame.x);
  expect(b.frame.x).toBe(d.frame.x);
});

test("a press updates the label", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  expect(app.texts()).toContain("Press");
  app.press("Press");
  expect(app.texts()).toContain("Pressed");
});

test("a static transform is mounted", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const shifted = holder(flat(app.tree(), []), "Shifted");
  expect(shifted.transform).toEqual([{ translateX: 12 }, { rotate: 15 }, { scale: 1.25 }]);
});

test("safe-area env insets pad the view", () => {
  const app = open("./App.tsx", {
    width: 390,
    height: 844,
    safeArea: { top: 22, right: 0, bottom: 0, left: 10 },
  });
  const inset = holder(flat(app.tree(), []), "Inset").children[0];
  expect(inset.frame.x).toBe(10);
  expect(inset.frame.y).toBe(22);
});

test("nested text is one block with distinct spans", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const block = flat(app.tree(), []).find((node) => node.text === "Plain bold italic color");
  expect(block.kind).toBe("text");
  const spans = block.spans;
  expect(spans.find((span) => span.text === " bold").fontWeight).toBe(700);
  expect(spans.find((span) => span.text === " italic").italic).toBe(true);
  const color = spans.find((span) => span.text === " color");
  expect(color.fontSize).toBe(22);
  expect(color.color).toBe("#c2410c");
});

test("images mount a bundled asset and a remote URL", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const bundled = nodes.find((node) => node.kind === "image" && String(node.src).indexOf("asset:/mark.png") === 0);
  expect(bundled.fit).toBe("cover");
  expect(bundled.frame.width).toBe(32);
  expect(bundled.frame.height).toBe(32);
  const remote = nodes.find((node) => node.src === "https://example.com/photo.jpg");
  expect(remote.kind).toBe("image");
  expect(remote.fit).toBe("contain");
  expect(remote.frame.width).toBe(48);
  expect(remote.frame.height).toBe(32);
});

test("scroll views overflow on each axis", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const down = nodes.find(
    (node) => node.kind === "scroll" && node.horizontal === false && node.content && node.content.height === 200,
  );
  const across = nodes.find((node) => node.kind === "scroll" && node.horizontal === true && containsText(node, "Across"));
  expect(down.frame.height).toBe(80);
  expect(down.content).toEqual({ width: 200, height: 200 });
  expect(across.frame.height).toBe(48);
  expect(across.frame.width).toBe(160);
  expect(across.content.width > across.frame.width).toBe(true);
  expect(across.content.height).toBe(48);
});

test("inputs take text, keyboards, a password, and a disabled value", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const input = (placeholder) => nodes.find((node) => node.kind === "input" && node.placeholder === placeholder);
  expect(input("Email").keyboard).toBe("email");
  expect(input("Amount").keyboard).toBe("number");
  expect(input("Phone").keyboard).toBe("phone");
  expect(input("Password").secure).toBe(true);
  expect(input("Password").keyboard).toBe("text");
  expect(input("Locked").disabled).toBe(true);
  expect(input("Locked").value).toBe("locked");
  expect(app.texts()).toContain("Locked: locked");
  reveal(app, (node) => node.kind === "input" && node.placeholder === "Name");
  app.type("Name", "ada");
  expect(app.texts()).toContain("Name: ada");
  expect(app.inputs()).toContain("ada");
  reveal(app, (node) => node.kind === "input" && node.placeholder === "Locked");
  expect(() => app.type("locked", "nope")).toThrow("disabled");
  expect(app.texts()).toContain("Locked: locked");
});

test("fetch renders the JSON", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  reveal(app, (node) => node.text === "Loading quote" || node.text === "Gallery: 5");
  app.waitFor("Gallery: 5");
});
