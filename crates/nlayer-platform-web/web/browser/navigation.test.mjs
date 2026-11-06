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

test("NavPage emits the bar's items for the host to draw", () => {
  const reactive = {
    signal(val) {
      const sig = () => val;
      sig.set = (next) => {
        val = next;
      };
      return sig;
    },
    computed: (fn) => () => fn(),
    untracked: (fn) => (typeof fn === "function" ? fn() : fn),
    batch: (fn) => fn(),
    context(def) {
      const read = () => def;
      read.Provider = (props) => props?.children;
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
    bindProp(el, name, value) {
      el.props[name] = value;
    },
    spread(el, props) {
      el.spread = props;
    },
    insert(parent, kids) {
      const list = Array.isArray(kids) ? kids : [kids];
      for (const kid of list) parent.kids.push(typeof kid === "function" ? kid() : kid);
    },
  };
  let presses = 0;
  const nav = createNavigation(
    reactive,
    { rt, listen: () => {} },
    { createAnimatedValue: () => ({}), Animated: { timing: () => ({}) } },
    () => ({ top: 47, right: 1, bottom: 20, left: 2 }),
    () => false,
  );

  const custom = rt.element("view");
  const titleView = rt.element("view");
  const page = nav.NavPage({
    title: "Hello",
    subtitle: "12 unread",
    largeTitleDisplayMode: "always",
    backButtonTitle: "Inbox",
    backButtonDisplayMode: "minimal",
    hidesBackButton: true,
    leftItemsSupplementBackButton: true,
    leadingItemGroups: [[{ title: "Edit", onPress: () => presses++ }], [custom]],
    trailingItemGroups: [[{ image: "asset:/plus.png", enabled: false }]],
    standardAppearance: { backgroundColor: "#fff" },
    titleView,
    children: "body",
  });

  const items = page.kids.filter((kid) => kid?._tag === "headeritem");
  const leading = items.filter((item) => item.props.headerSlot === "leading");
  assert.equal(leading.length, 2);
  assert.equal(leading[0].props.headerGroup, 0);
  assert.equal(leading[0].props.headerIndex, 0);
  assert.equal(leading[0].props.barButton, true);
  assert.equal(leading[0].props.barTitle, "Edit");
  leading[0].props.onPress();
  assert.equal(presses, 1);
  assert.equal(leading[1].props.headerGroup, 1);
  assert.equal(leading[1].kids[0], custom, "a custom view is the item's child");
  const trailing = items.filter((item) => item.props.headerSlot === "trailing");
  assert.equal(trailing.length, 1);
  assert.equal(trailing[0].props.barImage, "asset:/plus.png");
  assert.equal(trailing[0].props.barEnabled, false);
  const title = items.find((item) => item.props.headerSlot === "title");
  assert.equal(title.kids[0], titleView);
  assert.equal(
    tree.some((item) => item.props.headerSlot === "header"),
    false,
    "the app's tree holds no header slot",
  );
  // The options reach the core as screen props; the item groups do not.
  assert.equal(page.spread.title, "Hello");
  assert.equal(page.spread.subtitle, "12 unread");
  assert.equal(page.spread.largeTitleDisplayMode, "always");
  assert.equal(page.spread.backButtonTitle, "Inbox");
  assert.equal(page.spread.backButtonDisplayMode, "minimal");
  assert.equal(page.spread.hidesBackButton, true);
  assert.equal(page.spread.leftItemsSupplementBackButton, true);
  assert.equal(page.spread.leadingItemGroups, undefined);
  assert.equal(page.spread.titleView, undefined);
  assert.equal(page.spread.children, undefined);
});

test("NavPage spread keeps a live getter a getter", () => {
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
  let opaque = false;
  nav.NavPage({
    title: "Header",
    get standardAppearance() {
      return opaque ? { backgroundColor: "#fff" } : undefined;
    },
    children: "body",
  });
  assert.equal(spreadProps.standardAppearance, undefined);
  opaque = true;
  assert.equal(spreadProps.standardAppearance.backgroundColor, "#fff");
  assert.equal(Object.keys(spreadProps).includes("children"), false);
  assert.equal(spreadProps.title, "Header");
});
