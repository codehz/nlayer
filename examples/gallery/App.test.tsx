import { type App, type Node, expect, open, test } from "@nlayer/test";

function flat(node: Node, out: Node[]): Node[] {
  out.push(node);
  for (const child of node.children) flat(child, out);
  return out;
}

function containsText(node: Node, text: string): boolean {
  if (typeof node.text === "string" && node.text.includes(text)) return true;
  return node.children.some((child) => containsText(child, text));
}

function holder(nodes: Node[], text: string): Node {
  const found = nodes.find((node) => node.children.some((child) => child.text === text));
  if (!found) throw new Error(text);
  return found;
}

function locate(
  node: Node,
  pred: (node: Node) => boolean,
  top: number,
  scrollY: number,
): { node: Node; y: number } | null {
  const y = top + node.frame.y - scrollY;
  if (pred(node)) return { node, y };
  const childScroll = node.kind === "scroll" && node.offset ? node.offset.y : 0;
  for (const child of node.children) {
    const hit = locate(child, pred, y, childScroll);
    if (hit) return hit;
  }
  return null;
}

function reveal(app: App, pred: (node: Node) => boolean): void {
  const root = app.tree();
  const hit = locate(root, pred, 0, 0);
  const page = locate(root, (node) => node.testID === "page", 0, 0);
  if (!hit || !page) throw new Error("not on the page");
  const center = hit.y + hit.node.frame.height / 2;
  const mid = page.y + page.node.frame.height / 2;
  const distance = center - mid;
  if (distance > 1 || distance < -1) app.scroll("page", distance);
}

function must<T>(value: T | null | undefined): T {
  if (value == null) throw new Error("missing");
  return value;
}

test("column, row, and grid place their children", () => {
  const app = open("./App.tsx", {
    width: 390,
    height: 844,
    safeArea: { top: 22, right: 0, bottom: 0, left: 10 },
  });
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

test("pointer styles are paint and a press still lands", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const fade = holder(nodes, "Fade");
  const hover = holder(nodes, "Hover");
  const focus = must(nodes.find((node) => node.kind === "input" && node.placeholder === "Focus"));
  expect(fade.frame.width).toBe(160);
  expect(fade.frame.height).toBe(36);
  expect(hover.frame.width).toBe(120);
  expect(hover.frame.height).toBe(36);
  expect(focus.frame.width).toBe(160);
  expect(app.texts()).toContain("activeStyle={{ opacity: 0.4 }}");
  expect(app.texts()).toContain("activeStyle={{ backgroundColor }}");
  expect(app.texts()).toContain("hoverStyle={{ backgroundColor }}");
  expect(app.texts()).toContain("focusStyle={{ backgroundColor }}");
  reveal(app, (node) => node.children.some((child) => child.text === "Fade"));
  app.press("Fade");
  expect(app.texts()).toContain("Faded");
  reveal(app, (node) => node.children.some((child) => child.text === "Light"));
  app.press("Light");
  expect(app.texts()).toContain("Lit");
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
    safeArea: { top: 22, right: 8, bottom: 14, left: 10 },
    keyboard: 40,
  });
  const nodes = flat(app.tree(), []);
  const inset = holder(nodes, "Inset").children[0];
  expect(inset.frame.x).toBe(10);
  expect(inset.frame.y).toBe(22);
  const safe = must(nodes.find((node) => node.testID === "safe"));
  const title = must(safe.children[0]);
  expect(title.frame.x).toBe(26);
  expect(title.frame.y).toBe(38);
  const section = must(safe.children.find((node) => node.kind === "view"));
  expect(section.frame.x).toBe(26);
  expect(section.frame.x + section.frame.width).toBe(safe.frame.width - 24);
  const last = must(safe.children[safe.children.length - 1]);
  expect(safe.frame.height - (last.frame.y + last.frame.height)).toBe(72);
});

test("nested text is one block with distinct spans", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const block = must(flat(app.tree(), []).find((node) => node.text === "Plain bold italic color"));
  expect(block.kind).toBe("text");
  const spans = must(block.spans);
  expect(must(spans.find((span) => span.text === " bold")).fontWeight).toBe(700);
  expect(must(spans.find((span) => span.text === " italic")).italic).toBe(true);
  const color = must(spans.find((span) => span.text === " color"));
  expect(color.fontSize).toBe(22);
  expect(color.color).toBe("#c2410c");
});

test("images mount a bundled asset and a remote URL", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const bundled = must(
    nodes.find(
      (node) => node.kind === "image" && String(node.src).indexOf("asset:/mark.png") === 0,
    ),
  );
  expect(bundled.fit).toBe("cover");
  expect(bundled.frame.width).toBe(32);
  expect(bundled.frame.height).toBe(32);
  const remote = must(nodes.find((node) => node.src === "https://example.com/photo.jpg"));
  expect(remote.kind).toBe("image");
  expect(remote.fit).toBe("contain");
  expect(remote.frame.width).toBe(48);
  expect(remote.frame.height).toBe(32);
});

test("scroll views overflow on each axis", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const down = must(
    nodes.find(
      (node) =>
        node.kind === "scroll" &&
        node.horizontal === false &&
        node.content &&
        node.content.height === 200,
    ),
  );
  const across = must(
    nodes.find(
      (node) => node.kind === "scroll" && node.horizontal === true && containsText(node, "Across"),
    ),
  );
  expect(down.frame.height).toBe(80);
  expect(down.content).toEqual({ width: 200, height: 200 });
  expect(across.frame.height).toBe(48);
  expect(across.frame.width).toBe(160);
  expect(must(across.content).width > across.frame.width).toBe(true);
  expect(must(across.content).height).toBe(48);
  reveal(app, (node) => node.testID === "across");
  app.scroll("across", 40);
  const moved = must(flat(app.tree(), []).find((node) => node.testID === "across"));
  expect(moved.offset).toEqual({ x: 40, y: 0 });
});

test("scroll callbacks report the offset, content, and viewport", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const names = [
    "onScroll",
    "onScrollEnd",
    "onScrollDragStart",
    "onScrollDragEnd",
    "onMomentumStart",
    "onMomentumEnd",
  ];
  for (const name of names) {
    expect(app.texts()).toContain(`${name} none`);
  }
  reveal(app, (node) => node.testID === "events");
  app.scroll("events", 40);
  const events = must(flat(app.tree(), []).find((node) => node.testID === "events"));
  const offset = must(events.offset);
  const content = must(events.content);
  expect(offset).toEqual({ x: 0, y: 40 });
  const metric = `${String(Math.round(offset.x))},${String(Math.round(offset.y))} ${String(Math.round(content.width))}x${String(Math.round(content.height))} ${String(Math.round(events.frame.width))}x${String(Math.round(events.frame.height))}`;
  // Web's step is a wheel, which is not a drag. A finger's drag and glide
  // are covered by the browser input test (`web_test.rs`).
  const fired = globalThis.__nlayerOs === "web" ? names.slice(0, 2) : names;
  for (const name of fired) {
    app.waitFor(`${name} ${metric}`);
  }
});

test("inputs take text, keyboards, a password, and a disabled value", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const nodes = flat(app.tree(), []);
  const input = (placeholder: string) =>
    nodes.find((node) => node.kind === "input" && node.placeholder === placeholder)!;
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

test("platform.os is the surface", () => {
  const app = open("./App.tsx", { width: 390, height: 844 });
  const os = globalThis.__nlayerOs ?? "web";
  expect(["web", "ios", "android"]).toContain(os);
  const shown = app
    .texts()
    .filter((text) => text === "web" || text === "ios" || text === "android");
  expect(shown).toEqual([os]);
});
