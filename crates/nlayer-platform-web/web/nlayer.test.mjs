// The mounter against a document stand-in: frames, styles, text spans,
// images, one-axis scrolling, presses and a single-line input.

import assert from "node:assert/strict";
import test from "node:test";

import { Player } from "./nlayer.js";

class ClassList {
  constructor(el) {
    this.el = el;
  }
  add(...names) {
    for (const name of names) {
      if (name && !this.el._classes.includes(name)) this.el._classes.push(name);
    }
  }
  remove(...names) {
    this.el._classes = this.el._classes.filter((name) => !names.includes(name));
  }
  toggle(name, force) {
    const on = force ?? !this.contains(name);
    if (on) this.add(name);
    else this.remove(name);
    return on;
  }
  contains(name) {
    return this.el._classes.includes(name);
  }
}

class Element {
  constructor(doc, tag) {
    this.ownerDocument = doc;
    this.tagName = tag.toUpperCase();
    this._classes = [];
    this.classList = new ClassList(this);
    this.style = {};
    this.children = [];
    this.parentNode = null;
    this.listeners = {};
    this.attributes = {};
    this.scrollLeft = 0;
    this.scrollTop = 0;
    this.textContent = "";
    if (tag === "style") {
      this.sheet = { cssRules: [], insertRule: (rule, i) => {
        const entry = { cssText: rule, style: { cssText: "" } };
        const body = rule.slice(rule.indexOf("{") + 1, rule.lastIndexOf("}"));
        entry.style.cssText = body;
        this.sheet.cssRules.splice(i, 0, entry);
        return i;
      } };
    }
  }
  get className() {
    return this._classes.join(" ");
  }
  set className(value) {
    this._classes = value.split(/\s+/).filter(Boolean);
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
  dispatch(type) {
    const event = { stopPropagation() { event.stopped = true; } };
    for (const fn of this.listeners[type] ?? []) fn(event);
    if (!event.stopped) this.parentNode?.dispatch(type);
  }
  setAttribute(name, value) {
    this.attributes[name] = value;
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

class Document {
  constructor() {
    this.head = new Element(this, "head");
    this.documentElement = new Element(this, "html");
    this.ids = new Map();
  }
  createElement(tag) {
    return new Element(this, tag);
  }
  getElementById(id) {
    return this.ids.get(id) ?? null;
  }
}

function page() {
  const doc = new Document();
  const root = doc.createElement("div");
  const host = { presses: [], changes: [], scrolls: [] };
  host.press = (id) => host.presses.push(id);
  host.change = (id, text) => host.changes.push([id, text]);
  host.scroll = (id, x, y) => host.scrolls.push([id, x, y]);
  const player = new Player(root, host, { assetBase: "assets/" });
  player.doc = doc;
  return { doc, root, host, player };
}

test("a batch becomes views, text, an image, a scroll and an input", () => {
  const { root, host, player } = page();
  player.apply([
    ["c", 2, "view"],
    ["css", "nl-aaaaaaaa", "background-color:#ff0000;transform:rotate(45deg);"],
    ["css", "i2b", "border-top-width:1px;border-top-style:solid;border-top-color:#000000;"],
    ["s", 2, "nl-aaaaaaaa", "i2b"],
    ["f", 2, 0, 10, 100, 40],
    ["i", 1, 2, null],
    ["e", 2, ["onPress"]],
    ["c", 3, "text"],
    ["i", 2, 3, null],
    ["css", "i3t1", "font:italic 700 20px sans-serif;color:#ff0000;"],
    ["t", 3, 4, 2, 80, 18, null, false, [["Hello ", null], ["span", "i3t1"]]],
    ["c", 4, "image"],
    ["i", 1, 4, null],
    ["img", 4, "asset:/a.png", "contain"],
    ["c", 5, "scroll"],
    ["i", 1, 5, null],
    ["sc", 5, false],
    ["e", 5, ["onScroll"]],
    ["scs", 5, 100, 400],
    ["sb", 5, 0, 12],
    ["c", 6, "scroll"],
    ["sc", 6, true],
    ["c", 7, "input"],
    ["i", 1, 7, null],
    ["in", 7, null, { placeholder: "name", secure: false, keyboard: "email", disabled: false, rtl: false }],
    ["iv", 7, "hi", 0],
    ["ic", 7, 2, 3, 90, 18],
  ]);

  const view = player.view(2);
  assert.equal(view.style.top, "10px");
  assert.equal(view.style.width, "100px");
  assert.ok(view.className.includes("nl-aaaaaaaa"));
  assert.equal(view._border.className, "nl-border nl-p-i2b");
  assert.equal(view.children.at(-1), view._border);
  view.dispatch("click");
  assert.deepEqual(host.presses, [2]);

  const text = player.view(3)._text;
  assert.equal(text.style.left, "4px");
  assert.equal(text.children[1].className, "nl-p-i3t1");

  const img = player.view(4)._img;
  assert.equal(img.src, "assets/a.png");
  assert.ok(img.className.includes("nl-fit-contain"));

  const scroll = player.view(5);
  assert.ok(scroll.classList.contains("nl-oy"));
  assert.equal(scroll._spacer.style.height, "400px");
  assert.equal(scroll.scrollTop, 12);
  scroll.dispatch("scroll");
  assert.deepEqual(host.scrolls, [[5, 0, 12]]);
  player.apply([["cmd", 5, "scrollTo", 0, 30, false]]);
  assert.equal(scroll.scrollTop, 30);
  assert.ok(player.view(6).classList.contains("nl-ox"));

  const field = player.view(7)._field;
  assert.equal(field.placeholder, "name");
  assert.equal(field.type, "email");
  assert.equal(field.value, "hi");
  assert.equal(field.style.left, "2px");
  assert.equal(field.style.width, "90px");
  field.value = "hey";
  field.dispatch("input");
  assert.deepEqual(host.changes, [[7, "hey"]]);
  player.apply([["iv", 7, "stale", 0]]);
  assert.equal(field.value, "hey");
  player.apply([["iv", 7, "next", 1]]);
  assert.equal(field.value, "next");

  assert.equal(root.children.length > 0, true);
});
