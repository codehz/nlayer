// Selectors, the mounted tree, and press / type / scroll against the
// document stand-in. A browser is not involved: the stand-in dispatches
// the same click, input, and scroll the page listens to.

import assert from "node:assert/strict";
import test from "node:test";

import { installHost } from "./host.js";
import { Player } from "./nlayer.js";

function harness() {
  const doc = {
    createElement(tag) {
      return new El(doc, tag);
    },
    head: null,
    documentElement: null,
  };
  const root = new El(doc, "div");
  root._pin = { width: 100, height: 80 };
  const sink = { presses: [], changes: [], scrolls: [] };
  const player = new Player(root, {
    press(id) {
      sink.presses.push(id);
    },
    change(id, text) {
      sink.changes.push([id, text]);
    },
    scroll(id, kind, x, y) {
      sink.scrolls.push([id, kind, x, y]);
    },
  });
  player.doc = doc;
  const host = installHost(player, root);
  return { player, host, sink, root };
}

class El {
  constructor(doc, tag) {
    this.ownerDocument = doc;
    this.tagName = tag;
    this._classes = [];
    this.classList = {
      add: (...names) => names.forEach((name) => this._classes.push(name)),
      remove: () => {},
      toggle: () => {},
      contains: (name) => this._classes.includes(name),
    };
    this.style = {};
    this.children = [];
    this.parentNode = null;
    this.listeners = {};
    this.attributes = {};
    this.scrollLeft = 0;
    this.scrollTop = 0;
    this.textContent = "";
  }
  get className() {
    return this._classes.join(" ");
  }
  set className(value) {
    this._classes = String(value).split(/\s+/).filter(Boolean);
  }
  append(child) {
    this.insertBefore(child, null);
  }
  insertBefore(child, before) {
    child.remove();
    const at = before ? this.children.indexOf(before) : -1;
    if (at < 0) this.children.push(child);
    else this.children.splice(at, 0, child);
    child.parentNode = this;
  }
  remove() {
    this.parentNode?.children.splice(this.parentNode.children.indexOf(this), 1);
    this.parentNode = null;
  }
  replaceChildren() {
    for (const child of [...this.children]) child.remove();
  }
  addEventListener(type, fn) {
    (this.listeners[type] ??= []).push(fn);
  }
  dispatch(type, extra) {
    const event = {
      stopPropagation() {
        event.stopped = true;
      },
      ...(extra ?? {}),
    };
    for (const fn of this.listeners[type] ?? []) fn(event);
    if (!event.stopped) this.parentNode?.dispatch(type, extra);
  }
  setAttribute(name, value) {
    this.attributes[name] = value;
  }
  getAttribute(name) {
    return this.attributes[name];
  }
  removeAttribute(name) {
    delete this.attributes[name];
  }
  get src() {
    return this.attributes.src ?? "";
  }
  set src(value) {
    this.attributes.src = value;
  }
}

test("selectors name one view, and the steps use the page's events", () => {
  const { player, host, sink } = harness();
  player.apply([
    ["c", 2, "view"],
    ["f", 2, 0, 0, 80, 30],
    ["i", 1, 2, null],
    ["e", 2, ["onPress"]],
    ["tid", 2, "Save"],
    ["c", 3, "text"],
    ["f", 3, 0, 0, 40, 16],
    ["i", 2, 3, null],
    ["t", 3, 0, 0, 40, 16, null, false, [["Save", null]], [["label", 0, 4]], []],
    ["c", 4, "text"],
    ["f", 4, 0, 40, 40, 16],
    ["i", 1, 4, null],
    ["t", 4, 0, 0, 40, 16, null, false, [["Save", null]], [], []],
    ["c", 5, "input"],
    ["f", 5, 0, 20, 80, 20],
    ["i", 1, 5, null],
    ["tid", 5, "name"],
    [
      "in",
      5,
      null,
      { placeholder: "Name", secure: false, keyboard: "text", disabled: false, rtl: false },
    ],
    ["c", 6, "scroll"],
    ["f", 6, 0, 0, 100, 40],
    ["i", 1, 6, null],
    ["tid", 6, "list"],
    ["sc", 6, false],
    ["e", 6, ["onScroll"]],
    ["scs", 6, 100, 140],
    ["c", 7, "view"],
    ["f", 7, 0, 200, 10, 10],
    ["i", 1, 7, null],
    ["e", 7, ["onPress"]],
    ["tid", 7, "below"],
  ]);

  assert.deepEqual(host.texts(), ["Save", "Save"]);
  assert.deepEqual(host.inputs(), [""]);
  const tree = host.tree();
  assert.equal(tree.children.find((child) => child.testID === "Save").kind, "view");
  assert.equal(tree.children.find((child) => child.testID === "list").offset.y, 0);

  host.press("Save");
  assert.deepEqual(sink.presses, [2]);
  assert.throws(() => host.press("text:Save >> nth=2"), /does not take a press/);
  assert.throws(() => host.press("text:Save"), /nth=0/);
  assert.throws(() => host.press("missing"), /matches nothing/);
  assert.throws(() => host.press("below"), /not in view/);

  host.type("Name", "ab");
  assert.deepEqual(sink.changes, [
    [5, "a"],
    [5, "ab"],
  ]);
  assert.deepEqual(host.inputs(), ["ab"]);
  assert.equal(host.tree().children.find((child) => child.testID === "name").value, "ab");

  host.scroll("list", 30);
  assert.equal(player.view(6).scrollTop, 30);
  assert.deepEqual(sink.scrolls, [[6, "onScroll", 0, 30]]);
  host.scroll("list", 1000);
  assert.equal(player.view(6).scrollTop, 100);
  assert.equal(host.tree().children.find((child) => child.testID === "list").offset.y, 100);

  assert.equal(host.query("Save").state, "ready");
  assert.equal(host.query("missing").state, "wait");
  assert.equal(host.query("text:Save").state, "fail");
});

test("a span color is #rrggbb, not the browser's rgb()", () => {
  const { player, host, root } = harness();
  root.ownerDocument.defaultView = {
    getComputedStyle(el) {
      return {
        fontSize: "22px",
        fontWeight: "400",
        fontStyle: "normal",
        color: el.textContent === "fade" ? "rgba(194, 65, 12, 0.5)" : "rgb(194, 65, 12)",
      };
    },
  };
  player.apply([
    ["c", 2, "text"],
    ["i", 1, 2, null],
    [
      "t",
      2,
      0,
      0,
      80,
      16,
      null,
      false,
      [
        [" color", null],
        ["fade", null],
      ],
      [],
      [],
    ],
  ]);
  const spans = host.tree().children[0].spans;
  assert.equal(spans[0].color, "#c2410c");
  assert.equal(spans[1].color, "#c2410c80");
});

test("an image src is the engine URI, not the page URL", () => {
  const { player, host } = harness();
  player.assetBase = "./";
  player.apply([
    ["c", 2, "image"],
    ["i", 1, 2, null],
    ["img", 2, "asset:/mark.png", "cover"],
    ["c", 3, "image"],
    ["i", 1, 3, null],
    ["img", 3, "https://example.com/photo.jpg", "contain"],
  ]);
  assert.equal(player.view(2)._img.src, "./mark.png");
  const nodes = host.tree().children;
  assert.equal(nodes[0].src, "asset:/mark.png");
  assert.equal(nodes[0].fit, "cover");
  assert.equal(nodes[1].src, "https://example.com/photo.jpg");
  assert.equal(nodes[1].fit, "contain");
});
