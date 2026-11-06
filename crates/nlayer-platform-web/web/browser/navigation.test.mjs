// Simple navigation behavior tests for nlayer web platform

import assert from "node:assert/strict";
import test from "node:test";
import { createNavigation } from "./navigation.js";

test("createNavStack creates stack with routes", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };
  const rt = {
    element: (tag) => ({ _tag: tag }),
    prop: () => {},
    spread: () => {},
    insert: () => {},
    each: () => () => [],
    cond: () => () => null,
  };

  const dom = { rt, listen: () => {} };
  const animation = {
    createAnimatedValue: () => ({}),
    Animated: { timing: () => ({ start: () => {} }) },
  };
  const safeArea = () => ({ top: 0, right: 0, bottom: 0, left: 0 });
  const reduceMotion = () => false;

  const nav = createNavigation(reactive, dom, animation, safeArea, reduceMotion);

  const stack = nav.createNavStack([{ id: 1 }, { id: 2 }]);
  assert.equal(stack.routes().length, 2);

  stack.push({ id: 3 });
  assert.equal(stack.routes().length, 3);

  stack.pop();
  assert.equal(stack.routes().length, 2);
});

test("NavStack creates stack element", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };

  const rt = {
    element: (tag) => ({ _tag: tag }),
    prop: () => {},
    spread: () => {},
    insert: () => {},
    each: () => () => [],
  };

  const dom = { rt, listen: () => {} };
  const animation = {
    createAnimatedValue: () => ({}),
    Animated: { timing: () => ({ start: () => {} }) },
  };
  const safeArea = () => ({ top: 0, right: 0, bottom: 0, left: 0 });
  const reduceMotion = () => false;

  const nav = createNavigation(reactive, dom, animation, safeArea, reduceMotion);
  const stack = nav.createNavStack([{ id: 1 }, { id: 2 }]);

  const result = nav.NavStack({ state: stack, children: (route) => route });

  assert.ok(result, "NavStack should return element");
  assert.equal(result._tag, "stack", "Should create stack element");
});

test("Dialog closes on user action", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };

  const rt = {
    element: (tag) => ({ _tag: tag, _listeners: {} }),
    prop: () => {},
    bindProp: () => {},
    spread: () => {},
    insert: () => {},
    cond: (when, then) => () => (when() ? then() : null),
  };

  const dom = {
    rt,
    listen: (node, event, handler) => {
      node._listeners[event] = handler;
    },
  };
  const animation = {
    createAnimatedValue: () => ({}),
    Animated: { timing: () => ({ start: () => {} }) },
  };
  const safeArea = () => ({ top: 0, right: 0, bottom: 0, left: 0 });
  const reduceMotion = () => false;

  const nav = createNavigation(reactive, dom, animation, safeArea, reduceMotion);

  let openState = true;
  let closedReason = null;
  const dialog = nav.Dialog({
    open: openState,
    closeOnEscape: true,
    onOpenChange: ({ open, reason }) => {
      openState = open;
      closedReason = reason;
    },
    children: "content",
  });

  const modal = dialog();
  assert.ok(modal, "Dialog should render when open");
  assert.equal(modal._tag, "modal", "Should create modal element");

  modal._listeners.onRequestClose?.({ reason: "cancel" });
  assert.equal(openState, false, "Dialog should close");
  assert.equal(closedReason, "cancel", "Should report correct reason");
});

test("Dialog respects close flags", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };

  const rt = {
    element: (tag) => ({ _tag: tag, _listeners: {} }),
    prop: () => {},
    bindProp: () => {},
    spread: () => {},
    insert: () => {},
    cond: (when, then) => () => (when() ? then() : null),
  };

  const dom = {
    rt,
    listen: (node, event, handler) => {
      node._listeners[event] = handler;
    },
  };
  const animation = {
    createAnimatedValue: () => ({}),
    Animated: { timing: () => ({ start: () => {} }) },
  };
  const safeArea = () => ({ top: 0, right: 0, bottom: 0, left: 0 });
  const reduceMotion = () => false;

  const nav = createNavigation(reactive, dom, animation, safeArea, reduceMotion);

  let openState = true;
  const dialog = nav.Dialog({
    open: openState,
    closeOnEscape: false,
    onOpenChange: ({ open }) => {
      openState = open;
    },
    children: "content",
  });

  const modal = dialog();
  modal._listeners.onRequestClose?.({ reason: "cancel" });
  assert.equal(openState, true, "Dialog should not close when closeOnEscape is false");
});

test("NavPage provides insets context", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };

  let insetsCallback = null;
  const rt = {
    element: (tag) => ({ _tag: tag }),
    prop: (el, name, value) => {
      if (name === "onInsets") insetsCallback = value;
    },
    spread: () => {},
    insert: () => {},
  };

  const dom = { rt, listen: () => {} };
  const animation = {
    createAnimatedValue: () => ({}),
    Animated: { timing: () => ({ start: () => {} }) },
  };
  const safeArea = () => ({ top: 10, right: 0, bottom: 20, left: 0 });
  const reduceMotion = () => false;

  const nav = createNavigation(reactive, dom, animation, safeArea, reduceMotion);

  nav.NavPage({ children: "content" });

  assert.ok(insetsCallback, "Should register insets callback");
  insetsCallback({ top: 50, right: 0, bottom: 30, left: 0 });
});

test("Dialog slide starts a timing animation", () => {
  const reactive = {
    signal: (val) => {
      const sig = () => val;
      sig.set = (v) => {
        val = v;
      };
      sig.update = (fn) => {
        val = fn(val);
      };
      return sig;
    },
    computed: (fn) => fn,
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context: (def) => {
      const read = () => def;
      read.Provider = (props) => props?.children;
      return read;
    },
  };
  const rt = {
    element: (tag) => ({ _tag: tag, _listeners: {} }),
    prop: () => {},
    bindProp: () => {},
    spread: () => {},
    insert: () => {},
    cond: (when, then) => () => (when() ? then() : null),
  };
  const started = [];
  const animation = {
    createAnimatedValue: (value) => ({ value }),
    Animated: {
      timing(value, config) {
        started.push({ value, config });
        return { finished: true };
      },
    },
  };
  const nav = createNavigation(
    reactive,
    { rt, listen: () => {} },
    animation,
    () => ({ height: 480 }),
    () => false,
  );
  const dialog = nav.Dialog({
    open: true,
    animation: "slide",
    backdropStyle: { backgroundColor: "#00000066" },
    children: "content",
  });
  assert.ok(dialog());
  assert.equal(started.length, 1);
  assert.equal(started[0].config.toValue, 0);
  assert.equal(started[0].config.duration, 320);
  assert.equal(started[0].value.value, 480);
});

test("NavPage mounts the default header and can hide or replace it", () => {
  const reactive = {
    signal(val) {
      const subs = new Set();
      const sig = () => {
        subs.add(sig);
        return val;
      };
      sig.set = (next) => {
        val = next;
        for (const sub of subs) sub();
      };
      return sig;
    },
    computed(fn) {
      const read = () => fn();
      return read;
    },
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context(def) {
      let value = def;
      const read = () => value;
      read.Provider = (props) => {
        read.provided = props?.value;
        const previous = value;
        value = props?.value;
        const rendered = props?.children;
        value = previous;
        return rendered;
      };
      return read;
    },
  };
  const tree = [];
  const rt = {
    element(tag) {
      const el = { _tag: tag, props: {}, kids: [] };
      tree.push(el);
      return el;
    },
    prop(el, name, value) {
      el.props[name] = value;
    },
    bindProp() {},
    spread() {},
    insert(parent, kids) {
      const list = Array.isArray(kids) ? kids : [kids];
      for (const kid of list) parent.kids.push(typeof kid === "function" ? kid() : kid);
    },
  };
  let backs = 0;
  let scheme = "light";
  const nav = createNavigation(
    reactive,
    {
      rt,
      listen: () => {},
      requestBack: () => {
        backs += 1;
      },
      colorScheme: () => scheme,
    },
    { createAnimatedValue: () => ({}), Animated: { timing: () => ({}) } },
    () => ({ top: 47, right: 1, bottom: 20, left: 2 }),
    () => false,
  );

  const page = nav.NavPage({ title: "Hello", children: "content" });
  for (const kid of page.kids) if (typeof kid === "function") kid();
  const slot = tree.find((el) => el.props.headerSlot === "header");
  assert.ok(slot, "default header slot");
  const texts = tree.filter((el) => el._tag === "text").map((el) => el.kids.join(""));
  assert.ok(texts.includes("Hello"));
  assert.equal(
    tree.some((el) => el.props.accessibilityLabel === "Back"),
    false,
    "root page has no back control",
  );
  assert.equal(nav.insetsContext.provided().top, 0, "a visible header covers the top inset");

  tree.length = 0;
  const pushed = nav.NavHeader({
    title: "Detail",
    canGoBack: true,
    onBack: nav.setNavHeader && (() => backs++),
  });
  const pushedTexts = [];
  const walk = (node) => {
    if (!node || typeof node !== "object") return;
    if (node._tag === "text") pushedTexts.push(node.kids.join(""));
    for (const kid of node.kids ?? []) walk(kid);
  };
  walk(pushed);
  assert.ok(pushedTexts.includes("Detail"));
  assert.equal(pushedTexts.includes("←"), false);
  const back = tree.find((el) => el.props.accessibilityLabel === "Back");
  back.props.onPress();
  assert.equal(backs, 1);
  assert.equal(pushed.props.style.backgroundColor, "#ffffff");
  const row = pushed.kids[0];
  assert.equal(row.props.style.height, 44);
  assert.equal(row.props.style.borderBottomWidth, 0.5);
  assert.equal(row.props.style.borderBottomColor, "rgba(60,60,67,0.29)");
  const titleNode = tree.find((el) => el._tag === "text" && el.kids.join("") === "Detail");
  assert.equal(titleNode.props.style.color, "#000000");
  assert.equal(back._tag, "image");
  assert.match(decodeURIComponent(back.props.src), /M15 18 9 12 15 6/);
  assert.match(decodeURIComponent(back.props.src), /stroke="#007AFF"/);

  tree.length = 0;
  const clear = nav.NavHeader({
    title: "Clear",
    canGoBack: true,
    headerTransparent: true,
  });
  assert.equal(clear.props.style.backgroundColor, "transparent");
  assert.equal(clear.kids[0].props.style.borderBottomWidth, 0);
  const clearTitle = tree.find((el) => el._tag === "text" && el.kids.join("") === "Clear");
  const clearBack = tree.find((el) => el.props.accessibilityLabel === "Back");
  assert.notEqual(clearTitle.props.style.color, "#ffffff");
  assert.match(decodeURIComponent(clearBack.props.src), /stroke="#007AFF"/);
  assert.equal(clearTitle.props.style.color, "#000000");

  scheme = "dark";
  tree.length = 0;
  const dark = nav.NavHeader({ title: "Night", canGoBack: true });
  assert.equal(dark.props.style.backgroundColor, "#000000");
  const darkTitle = tree.find((el) => el._tag === "text" && el.kids.join("") === "Night");
  const darkBack = tree.find((el) => el.props.accessibilityLabel === "Back");
  assert.equal(darkTitle.props.style.color, "#ffffff");
  assert.match(decodeURIComponent(darkBack.props.src), /stroke="#0A84FF"/);

  tree.length = 0;
  nav.setNavHeader(() => {
    const el = rt.element("view");
    el.props.marker = "custom";
    return el;
  });
  const custom = nav.NavPage({ title: "Custom", children: "body" });
  for (const kid of custom.kids) if (typeof kid === "function") kid();
  assert.ok(tree.some((el) => el.props.marker === "custom"));

  tree.length = 0;
  nav.setNavHeader(null);
  const hidden = nav.NavPage({ title: "Hidden", headerVisible: false, children: "body" });
  for (const kid of hidden.kids) if (typeof kid === "function") kid();
  assert.equal(
    tree.some((el) => el.props.headerSlot === "header"),
    false,
  );
  assert.equal(
    nav.insetsContext.provided().top,
    47,
    "no header: the content inset is the safe area",
  );
  assert.throws(() => nav.setNavHeader("nope"), /setNavHeader/);
});

test("NavPage spread keeps a headerTransparent getter live", () => {
  const reactive = {
    signal(val) {
      const sig = () => val;
      sig.set = (next) => {
        val = next;
      };
      return sig;
    },
    computed(fn) {
      return () => fn();
    },
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context(def) {
      let value = def;
      const read = () => value;
      read.Provider = (props) => {
        const previous = value;
        value = props?.value;
        const rendered = props?.children;
        value = previous;
        return rendered;
      };
      return read;
    },
  };
  let spreadProps = null;
  const rt = {
    element: (tag) => ({ _tag: tag, props: {}, kids: [] }),
    prop() {},
    bindProp() {},
    spread(_node, props) {
      spreadProps = props;
    },
    insert() {},
  };
  const nav = createNavigation(
    reactive,
    { rt, listen: () => {}, colorScheme: () => "light" },
    { createAnimatedValue: () => ({}), Animated: { timing: () => ({}) } },
    () => ({ top: 47, right: 0, bottom: 34, left: 0 }),
    () => false,
  );
  let transparent = false;
  nav.NavPage({
    title: "Header",
    get headerTransparent() {
      return transparent;
    },
    children: "body",
  });
  assert.equal(spreadProps.headerTransparent, false);
  transparent = true;
  assert.equal(spreadProps.headerTransparent, true);
  assert.equal(Object.keys(spreadProps).includes("children"), false);
  assert.equal(spreadProps.title, "Header");
});
