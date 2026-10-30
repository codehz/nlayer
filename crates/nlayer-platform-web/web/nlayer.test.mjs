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
      this.sheet = {
        cssRules: [],
        insertRule: (rule, i) => {
          const entry = { cssText: rule, style: { cssText: "" } };
          const body = rule.slice(rule.indexOf("{") + 1, rule.lastIndexOf("}"));
          entry.style.cssText = body;
          this.sheet.cssRules.splice(i, 0, entry);
          return i;
        },
      };
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
  dispatch(type, extra) {
    const event = {
      stopPropagation() {
        event.stopped = true;
      },
      ...(extra ?? {}),
      target: extra?.target ?? this,
    };
    for (const fn of this.listeners[type] ?? []) fn(event);
    if (!event.stopped) this.parentNode?.dispatch(type, { ...extra, target: event.target });
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
  const host = { presses: [], changes: [], scrolls: [], selections: [], compositions: [] };
  host.press = (id) => host.presses.push(id);
  host.change = (id, text) => host.changes.push([id, text]);
  host.scroll = (...args) => host.scrolls.push(args);
  host.selection = (id, start, end) => host.selections.push([id, start, end]);
  host.composition = (id, composing, text) => host.compositions.push([id, composing, text]);
  const player = new Player(root, host, { assetBase: "assets/" });
  player.doc = doc;
  return { doc, root, host, player };
}

test("packaged assets use mapped names while unmapped URLs retain their paths", () => {
  const { player } = page();
  player.assetMap = { "img/icon.png": "hashed/icon.png" };
  assert.equal(player.asset("asset:/img/icon.png"), "assets/hashed/icon.png");
  assert.equal(player.asset("asset:/img/other.png"), "assets/img/other.png");
  assert.equal(player.asset("https://example.com/icon.png"), "https://example.com/icon.png");
});

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
    [
      "t",
      3,
      4,
      2,
      80,
      18,
      null,
      false,
      [
        ["Hello ", null],
        ["span", "i3t1"],
      ],
    ],
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
    [
      "in",
      7,
      null,
      { placeholder: "name", secure: false, keyboard: "email", disabled: false, rtl: false },
    ],
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
  assert.equal(img.style.borderRadius, "inherit");
  assert.equal(img.style.overflow, "clip");

  const scroll = player.view(5);
  assert.ok(scroll.classList.contains("nl-oy"));
  assert.equal(scroll._spacer.style.height, "400px");
  assert.equal(scroll.scrollTop, 12);
  scroll.dispatch("scroll");
  assert.deepEqual(host.scrolls, [
    [5, "onScroll", 0, 12, undefined, undefined, undefined, undefined],
  ]);
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

test("active, hover and focus styles are rules on the view", () => {
  const { player } = page();
  player.apply([
    ["c", 2, "view"],
    ["i", 1, 2, null],
    ["rule", "i2a", 4, ":active", "", "background-color:#ff0000;"],
    ["rule", "i2a", 4, ":active", " > .nl-border", ""],
    ["rule", "i2a", 4, ":active", " .nl-text span", "color:#00ff00;"],
    ["rule", "i2a", 4, ":active", " > .nl-field", "color:#00ff00;"],
    ["st", 2, "active", "i2a"],
    ["rule", "i2f", 2, ":focus-within", "", ""],
    ["rule", "i2f", 2, ":focus-within", " > .nl-border", ""],
    ["rule", "i2f", 2, ":focus-within", " .nl-text span", ""],
    ["rule", "i2f", 2, ":focus-within", " > .nl-field", ""],
    ["st", 2, "focus", "i2f"],
  ]);
  const view = player.view(2);
  assert.ok(view.classList.contains("nl-p-i2a"));
  assert.ok(view.classList.contains("nl-p-i2f"));
  assert.equal(view.tabIndex, 0);
  const sheet = player.doc.head.children.find((el) => el.id === "nl-states");
  const rules = sheet.sheet.cssRules.map((rule) => rule.cssText);
  assert.ok(
    rules.some(
      (rule) =>
        rule.startsWith(".nl-p-i2a.nl-p-i2a.nl-p-i2a.nl-p-i2a:active:not(.nl-disabled){") &&
        rule.includes("background-color:#ff0000"),
    ),
  );
  assert.ok(rules.some((rule) => rule.includes(".nl-text span") && rule.includes("color:#00ff00")));
  player.apply([
    ["rule", "i2a", 4, ":active", "", ""],
    ["st", 2, "active", null],
    ["st", 2, "focus", null],
  ]);
  assert.equal(view.classList.contains("nl-p-i2a"), false);
  assert.equal(view.tabIndex, undefined);

  player.apply([
    ["c", 3, "input"],
    [
      "in",
      3,
      null,
      { placeholder: "", secure: false, keyboard: "text", disabled: true, rtl: false },
    ],
  ]);
  assert.ok(player.view(3).classList.contains("nl-disabled"));
});

test("a click on a pressable span presses that span", () => {
  const { host, player } = page();
  player.apply([
    ["c", 2, "text"],
    ["t", 2, 0, 0, 80, 16, null, false, [["see "], ["docs"]], [], [[4, 8, 9]]],
    ["f", 2, 0, 0, 80, 16],
  ]);
  player.view(2).dispatch("click", { _utf16: 5 });
  assert.deepEqual(host.presses, [9]);
  player.view(2).dispatch("click", { _utf16: 1 });
  assert.deepEqual(host.presses, [9]);
});

const SCROLL_EVENTS = [
  "onScroll",
  "onScrollEnd",
  "onScrollDragStart",
  "onScrollDragEnd",
  "onMomentumStart",
  "onMomentumEnd",
];

function scrollPage() {
  const harness = page();
  harness.player.apply([
    ["c", 5, "scroll"],
    ["i", 1, 5, null],
    ["sc", 5, false],
    ["e", 5, SCROLL_EVENTS],
    ["f", 5, 0, 0, 80, 40],
    ["scs", 5, 80, 400],
  ]);
  const el = harness.player.view(5);
  el.clientWidth = 80;
  el.clientHeight = 40;
  el.scrollWidth = 80;
  el.scrollHeight = 400;
  return { ...harness, el };
}

function names(host) {
  return host.scrolls.map((call) => call[1]);
}

test("a wheel scroll reports onScroll and ends at rest", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, el } = scrollPage();
  el.scrollTop = 18;
  el.dispatch("scroll");
  el.dispatch("scrollend");
  assert.deepEqual(names(host), ["onScroll", "onScrollEnd"]);
  assert.deepEqual(host.scrolls[0], [5, "onScroll", 0, 18, 80, 400, 80, 40]);
  assert.equal(host.scrolls[1][1], "onScrollEnd");
  t.mock.timers.tick(200);
  assert.deepEqual(names(host), ["onScroll", "onScrollEnd"]);
});

test("quiet scrolling ends once without scrollend", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, el } = scrollPage();
  el.scrollTop = 9;
  el.dispatch("scroll");
  t.mock.timers.tick(200);
  assert.deepEqual(names(host), ["onScroll", "onScrollEnd"]);
  t.mock.timers.tick(200);
  assert.deepEqual(names(host), ["onScroll", "onScrollEnd"]);
});

test("a touch drag flings, and a new drag catches the glide", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, player, el } = scrollPage();
  let now = 1000;
  player.now = () => now;
  const touch = { pointerId: 1, pointerType: "touch", clientX: 4, clientY: 4 };
  el.dispatch("pointerdown", touch);
  el.dispatch("pointercancel", { ...touch, clientY: 30 });
  el.scrollTop = 20;
  el.dispatch("scroll");
  el.dispatch("touchend");
  now += 40;
  el.scrollTop = 48;
  el.dispatch("scroll");
  assert.deepEqual(names(host), [
    "onScrollDragStart",
    "onScroll",
    "onScrollDragEnd",
    "onMomentumStart",
    "onScroll",
  ]);
  el.dispatch("pointerdown", touch);
  el.dispatch("scrollend");
  el.dispatch("pointercancel", { ...touch, clientY: 20 });
  assert.deepEqual(names(host).slice(5), ["onMomentumEnd", "onScrollDragStart"]);
});

test("a drag that does not glide ends without momentum", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, player, el } = scrollPage();
  player.now = () => 0;
  const touch = { pointerId: 1, pointerType: "touch", clientX: 1, clientY: 1 };
  el.dispatch("pointerdown", touch);
  el.dispatch("pointercancel", { ...touch, clientY: 20 });
  el.dispatch("touchend");
  t.mock.timers.tick(200);
  assert.deepEqual(names(host), ["onScrollDragStart", "onScrollDragEnd", "onScrollEnd"]);
});

test("a vertical drag hits the vertical ancestor, and a mouse does not drag", () => {
  const { host, player } = page();
  player.apply([
    ["c", 5, "scroll"],
    ["i", 1, 5, null],
    ["sc", 5, false],
    ["e", 5, ["onScrollDragStart"]],
    ["c", 6, "scroll"],
    ["i", 5, 6, null],
    ["sc", 6, true],
    ["e", 6, ["onScrollDragStart"]],
    ["c", 7, "view"],
    ["i", 6, 7, null],
  ]);
  const inner = player.view(7);
  const touch = { pointerId: 3, pointerType: "touch", clientX: 8, clientY: 8 };
  inner.dispatch("pointerdown", touch);
  inner.dispatch("pointercancel", { ...touch, clientY: 40 });
  assert.deepEqual(host.scrolls, [
    [5, "onScrollDragStart", 0, 0, undefined, undefined, undefined, undefined],
  ]);
  host.scrolls.length = 0;
  inner.dispatch("pointerdown", { ...touch, pointerType: "mouse" });
  inner.dispatch("pointercancel", { ...touch, pointerType: "mouse", clientY: 40 });
  assert.deepEqual(host.scrolls, []);
});

test("onScroll is once per frame, and an instant command is not an end", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { doc, host, player, el } = scrollPage();
  const frames = [];
  doc.defaultView = {
    requestAnimationFrame(fn) {
      frames.push(fn);
      return frames.length;
    },
  };
  el.scrollTop = 4;
  el.dispatch("scroll");
  el.scrollTop = 11;
  el.dispatch("scroll");
  assert.deepEqual(host.scrolls, []);
  frames[0]();
  assert.deepEqual(host.scrolls, [[5, "onScroll", 0, 11, 80, 400, 80, 40]]);
  host.scrolls.length = 0;
  doc.defaultView = null;
  player.apply([["sb", 5, 0, 3]]);
  el.dispatch("scroll");
  el.dispatch("scrollend");
  assert.deepEqual(names(host), ["onScroll"]);
  player.apply([["cmd", 5, "scrollTo", 0, 30, true]]);
  el.scrollTop = 30;
  el.dispatch("scroll");
  el.dispatch("scrollend");
  assert.deepEqual(names(host), ["onScroll", "onScroll", "onScrollEnd"]);
});

test("a tap in a scroll blurs, and never swallows the press", () => {
  const { doc, host, player } = page();
  player.apply([
    ["c", 5, "scroll"],
    ["i", 1, 5, null],
    ["sc", 5, false, "never", "none"],
    ["c", 2, "view"],
    ["i", 5, 2, null],
    ["e", 2, ["onPress"]],
    ["c", 7, "input"],
    ["i", 5, 7, null],
    [
      "in",
      7,
      null,
      { placeholder: "", secure: false, keyboard: "text", disabled: false, rtl: false },
    ],
  ]);
  const scroll = player.view(5);
  const row = player.view(2);
  const field = player.view(7)._field;
  field.blur = () => {
    field.blurred = true;
    doc.activeElement = null;
  };
  doc.activeElement = field;
  player.setKeyboard(0);
  row.dispatch("pointerdown", { button: 0, clientX: 1, clientY: 1 });
  assert.equal(field.blurred, undefined);
  player.setKeyboard(300);
  row.dispatch("pointerdown", { button: 0, clientX: 1, clientY: 1 });
  assert.equal(field.blurred, true);
  row.dispatch("click");
  assert.deepEqual(host.presses, []);
  field.blurred = false;
  doc.activeElement = field;
  player.apply([["sc", 5, false, "always", "on-drag"]]);
  row.dispatch("pointerdown", { button: 0, clientX: 1, clientY: 1 });
  row.dispatch("click");
  assert.equal(field.blurred, false);
  assert.deepEqual(host.presses, [2]);
  doc.activeElement = field;
  field.dispatch("keydown", { key: "Enter", preventDefault() {} });
  assert.equal(field.blurred, true);
});

test("handled dismisses only off a press and an input", () => {
  const { doc, host, player } = page();
  player.apply([
    ["c", 5, "scroll"],
    ["i", 1, 5, null],
    ["sc", 5, false, "handled", "none"],
    ["e", 5, ["onPress"]],
    ["c", 3, "text"],
    ["i", 5, 3, null],
    ["t", 3, 0, 0, 80, 16, null, false, [["see "], ["docs"]], [], [[4, 8, 9]]],
    ["c", 7, "input"],
    ["i", 5, 7, null],
    [
      "in",
      7,
      null,
      { placeholder: "", secure: false, keyboard: "text", disabled: false, rtl: false },
    ],
    ["c", 8, "view"],
    ["i", 5, 8, null],
  ]);
  const scroll = player.view(5);
  const text = player.view(3);
  const blank = player.view(8);
  const field = player.view(7)._field;
  field.blur = () => {
    field.blurred = true;
    doc.activeElement = null;
  };
  const arm = () => {
    field.blurred = false;
    doc.activeElement = field;
    player.setKeyboard(300);
  };

  arm();
  scroll.dispatch("pointerdown", { button: 0, target: scroll });
  scroll.dispatch("click", { target: scroll });
  assert.equal(field.blurred, false);
  assert.deepEqual(host.presses, [5]);

  player.apply([["e", 5, []]]);

  arm();
  text.dispatch("pointerdown", { button: 0, _utf16: 1 });
  assert.equal(field.blurred, true);

  arm();
  text.dispatch("pointerdown", { button: 0, _utf16: 5 });
  assert.equal(field.blurred, false);

  arm();
  field.dispatch("pointerdown", { button: 0 });
  assert.equal(field.blurred, false);

  arm();
  blank.dispatch("pointerdown", { button: 0 });
  assert.equal(field.blurred, true);
});

test("a multiline field holds a composition and reports its selection", () => {
  const { host, player } = page();
  player.apply([
    ["c", 2, "input"],
    ["i", 1, 2, null],
    [
      "in",
      2,
      null,
      {
        placeholder: "body",
        secure: false,
        keyboard: "text",
        disabled: false,
        rtl: false,
        multiline: true,
      },
    ],
    ["iv", 2, "ab", 0],
  ]);
  let field = player.view(2)._field;
  assert.equal(field.tagName, "TEXTAREA");
  assert.equal(field.value, "ab");

  field.selectionStart = 1;
  field.selectionEnd = 2;
  field.dispatch("select");
  assert.deepEqual(host.selections, [[2, 1, 2]]);

  // A composition reports its text, and the value JS writes meanwhile waits.
  field.dispatch("compositionstart", { data: "zhong" });
  field.value = "a中b";
  field.dispatch("input");
  assert.deepEqual(host.compositions, [[2, true, "zhong"]]);
  player.apply([["iv", 2, "held", 1]]);
  assert.equal(field.value, "a中b");
  field.dispatch("compositionend");
  assert.deepEqual(host.compositions, [
    [2, true, "zhong"],
    [2, false, ""],
  ]);
  assert.equal(field.value, "held");

  // Back to one line: the editor is replaced and keeps the text.
  player.apply([
    [
      "in",
      2,
      null,
      {
        placeholder: "body",
        secure: false,
        keyboard: "text",
        disabled: false,
        rtl: false,
        multiline: false,
      },
    ],
  ]);
  field = player.view(2)._field;
  assert.equal(field.tagName, "INPUT");
  assert.equal(field.value, "held");
});

test("setSelection moves the caret without reporting it back", () => {
  const { host, player } = page();
  player.apply([
    ["c", 2, "input"],
    ["i", 1, 2, null],
    [
      "in",
      2,
      null,
      {
        placeholder: "",
        secure: false,
        keyboard: "text",
        disabled: false,
        rtl: false,
        multiline: false,
      },
    ],
    ["iv", 2, "hello", 0],
  ]);
  const field = player.view(2)._field;
  field.setSelectionRange = (start, end) => {
    field.selectionStart = start;
    field.selectionEnd = end;
    field.dispatch("select");
  };
  player.apply([["cmd", 2, "setSelection", 1, 3, false]]);
  assert.equal(field.selectionStart, 1);
  assert.equal(field.selectionEnd, 3);
  assert.deepEqual(host.selections, [], "the controlled range is not a user move");
});
