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
