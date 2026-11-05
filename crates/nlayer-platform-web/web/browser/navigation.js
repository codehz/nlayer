// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// Web navigation: reactive route stack, screen and modal DOM mounting,
// the default header bar, modal presentation and history integration.
// Adapted from nsh's browser navigation facade to nlayer's simpler
// environment model.

// Route state is held by the shared Rust reactive graph. Stack transitions,
// layout, back handling and modal surfaces remain in the Rust core/platform.
export function createNavigation(reactive, dom, animation, safeArea, reduceMotion) {
  const { signal, computed, untracked, batch, cleanup } = reactive;
  const rt = dom.rt;
  const pages = new WeakSet();
  const popped = new WeakMap();
  let headerComponent = null;
  function currentHeader() {
    headerComponent ??= signal(null);
    return headerComponent();
  }
  const stackContext = reactive.context(null);
  const colorScheme = dom.colorScheme ?? (() => "light");
  const requestBack = dom.requestBack ?? (() => {});
  // Descriptors, not values: a getter stays a getter (`merge` does the
  // same). Copying `headerTransparent` once froze the screen prop, so the
  // bar restyled on the first toggle while the content inset did not.
  const except = (props, keys) => {
    const out = {};
    for (const key of Object.keys(props)) {
      if (keys.includes(key)) continue;
      const desc = Object.getOwnPropertyDescriptor(props, key);
      if (desc) Object.defineProperty(out, key, desc);
    }
    return out;
  };

  function createNavStack(routes) {
    if (!Array.isArray(routes)) throw new TypeError("createNavStack: an array of routes");
    const entries = signal(routes.map((route) => ({ route })));
    const edit = (fn) => {
      const next = untracked(entries).slice();
      fn(next);
      entries.set(next);
    };
    const state = {
      routes: computed(() => entries().map((entry) => entry.route)),
      push(route) {
        edit((next) => next.push({ route }));
      },
      pop(count = 1) {
        edit((next) =>
          next.splice(Math.max(1, next.length - Math.trunc(Math.max(0, Number(count)) || 0))),
        );
      },
      popToRoot() {
        edit((next) => next.splice(1));
      },
      replace(route) {
        edit((next) => {
          next.pop();
          next.push({ route });
        });
      },
      setRoutes(routes) {
        const old = untracked(entries).slice();
        entries.set(
          (Array.isArray(routes) ? routes : []).map((route) => {
            const index = old.findIndex((entry) => entry && entry.route === route);
            if (index < 0) return { route };
            const entry = old[index];
            old[index] = null;
            return entry;
          }),
        );
      },
      remove(route) {
        edit((next) => {
          const index = next.findLastIndex((entry) => entry.route === route);
          if (index >= 0) next.splice(index, 1);
        });
      },
    };
    Object.defineProperty(state, "__nlayerEntries", { value: entries });
    return state;
  }

  function nativeInsets(coverTop) {
    const covers = typeof coverTop === "function" ? coverTop : () => coverTop;
    const reported = signal(null);
    return [
      computed(() => {
        const safe = safeArea();
        if (reported()) return reported();
        return covers() ? { ...safe, top: 0 } : safe;
      }),
      ({ top, right, bottom, left }) => reported.set({ top, right, bottom, left }),
    ];
  }

  const insetsContext = reactive.context({ top: 0, right: 0, bottom: 0, left: 0 });
  const withInsets = (insets, children) => {
    const Provider = insetsContext.Provider;
    return Provider({
      value: insets,
      get children() {
        return children();
      },
    });
  };

  function setNavHeader(component) {
    if (component != null && typeof component !== "function") {
      throw new TypeError("setNavHeader: a component function, or null");
    }
    headerComponent ??= signal(null);
    headerComponent.set(component ?? null);
  }

  const LIGHT_BAR = { bg: "#ffffff", title: "#0f172a", tint: "#3b82f6", line: "#e2e8f0" };
  const DARK_BAR = { bg: "#1e293b", title: "#f1f5f9", tint: "#60a5fa", line: "#334155" };

  function absent(value) {
    return value == null || value === false;
  }

  function backControl(tint, onBack) {
    const node = rt.element("text");
    rt.prop(node, "class", "font-semibold");
    rt.prop(node, "style", { color: tint });
    rt.prop(node, "accessibilityLabel", "Back");
    rt.prop(node, "onPress", () => onBack?.());
    rt.insert(node, "←");
    return node;
  }

  function titleView(title, color) {
    if (title != null && typeof title === "object") return title;
    const node = rt.element("text");
    rt.prop(node, "class", "shrink-0 px-2 text-center text-[15px] font-semibold");
    rt.prop(node, "style", { color, numberOfLines: 1 });
    rt.insert(node, title == null ? "" : String(title));
    return node;
  }

  function side(end, content) {
    const node = rt.element("view");
    rt.prop(
      node,
      "class",
      end
        ? "min-w-0 flex-1 flex-row items-center justify-end px-3"
        : "min-w-0 flex-1 flex-row items-center px-3",
    );
    if (content != null) rt.insert(node, content);
    return node;
  }

  function NavHeader(props) {
    const scheme = colorScheme() === "dark" ? DARK_BAR : LIGHT_BAR;
    const transparent = !!props.headerTransparent;
    const headerStyle = props.headerStyle ?? {};
    const background = transparent ? "transparent" : (headerStyle.backgroundColor ?? scheme.bg);
    const titleColor = headerStyle.color ?? (transparent ? "#ffffff" : scheme.title);
    const tint = headerStyle.tintColor ?? (transparent ? "#ffffff" : scheme.tint);
    const hairline = !transparent && headerStyle.backgroundColor == null;
    const customLeading = !absent(props.headerLeft);
    const showBack = !!props.canGoBack && props.headerBackVisible !== false && !customLeading;
    const leading = customLeading
      ? props.headerLeft
      : showBack
        ? backControl(tint, props.onBack)
        : null;
    const title = absent(props.headerTitle) ? (props.title ?? "") : props.headerTitle;
    const trailing = absent(props.headerRight) ? null : props.headerRight;

    const root = rt.element("view");
    rt.prop(root, "style", {
      paddingTop: "env(safe-area-inset-top)",
      backgroundColor: background,
    });
    const row = rt.element("view");
    rt.prop(row, "class", "h-11 flex-row items-center");
    rt.prop(row, "style", {
      borderBottomWidth: hairline ? 1 : 0,
      borderBottomColor: scheme.line,
    });
    rt.insert(row, [side(false, leading), titleView(title, titleColor), side(true, trailing)]);
    rt.insert(root, row);
    return root;
  }

  function NavPage(props) {
    const node = rt.element("screen");
    pages.add(node);
    // popOnBack must be reactive too (separate from spread): the gallery
    // computes it from detailInterceptBack signal. `header` is not a prop
    // anymore: spreading it warns. The slot props are the header's children.
    const restProps = except(props, [
      "children",
      "headerLeft",
      "headerRight",
      "headerTitle",
      "onDismiss",
      "onBackPress",
      "onInsets",
      "popOnBack",
    ]);
    rt.spread(node, restProps);
    if ("popOnBack" in props) {
      rt.bindProp(node, "popOnBack", () => props.popOnBack);
    }
    const [insets, onInsets] = nativeInsets(() => props.headerVisible !== false);
    rt.prop(node, "onInsets", onInsets);
    // `prop`, not `listen`: the engine only emits Dismiss to a screen that
    // told it about the event, and back depends on it.
    rt.prop(node, "onDismiss", () => {
      popped.get(node)?.();
      props.onDismiss?.();
    });
    // Set only when the page passes one (nsh). `bindProp` keeps the
    // engine's listener in step when the handler later appears or goes.
    if ("onBackPress" in props) {
      rt.bindProp(node, "onBackPress", () => props.onBackPress ?? null);
    }
    const header = computed(() => {
      if (props.headerVisible === false) return null;
      const Component = currentHeader() ?? NavHeader;
      const stack = stackContext();
      const canGoBack = !!(stack && stack.index() > 0);
      // A scheme change rebuilds the bar. The build itself is untracked.
      colorScheme();
      const model = {
        title: props.title,
        headerLeft: props.headerLeft,
        headerRight: props.headerRight,
        headerTitle: props.headerTitle,
        headerBackVisible: props.headerBackVisible,
        headerStyle: props.headerStyle,
        headerTransparent: props.headerTransparent,
        canGoBack,
        onBack: requestBack,
      };
      return untracked(() => {
        const item = rt.element("headeritem");
        rt.prop(item, "headerSlot", "header");
        rt.insert(item, Component(model));
        return item;
      });
    });
    rt.insert(node, [header, withInsets(insets, () => props.children)]);
    return node;
  }

  function NavStack(props) {
    const node = rt.element("stack");
    rt.spread(node, except(props, ["state", "children"]));
    const state = props.state;
    const entries = state.__nlayerEntries;
    // The route list lives here, so back pops it here: the engine's own
    // stack never sees these screens.
    const off = dom.back?.(() => {
      if (untracked(entries).length <= 1) return false;
      state.pop();
      return true;
    });
    if (off) cleanup(off);
    const rows = rt.each(entries, (read, index) => {
      const entry = untracked(read);
      // The children callback must be reactive: the gallery computes popOnBack
      // from signals, and NavPage props need to update when those change.
      return computed(() => {
        const rendered = stackContext.Provider({
          value: { index },
          get children() {
            return props.children(entry.route, index);
          },
        });
        return untracked(() => {
          const screenFor = (rendered) => {
            const page = pages.has(rendered) ? rendered : NavPage({ children: rendered });
            popped.set(page, () =>
              entries.set(untracked(entries).filter((value) => value !== entry)),
            );
            return page;
          };
          return typeof rendered === "function"
            ? computed(() => {
                const value = rendered();
                return untracked(() => screenFor(value));
              })
            : screenFor(rendered);
        });
      });
    });
    rt.insert(node, rows);
    return node;
  }

  function Dialog(props) {
    let closedByUser = false;
    const tick = signal(0);
    const close = (reason) => {
      props.onCancel?.();
      const flag = {
        backdropPress: "closeOnBackdropPress",
        swipe: "closeOnSwipe",
        cancel: "closeOnEscape",
      }[reason];
      if (props[flag] === false || closedByUser) return;
      batch(() => {
        props.onOpenChange?.({ open: false, reason });
        closedByUser = true;
        tick.update((n) => n + 1);
      });
    };
    return computed(() => {
      tick();
      if (props.open === false) {
        closedByUser = false;
        return null;
      }
      if (closedByUser) return null;
      return untracked(() => {
        const node = rt.element("modal");
        rt.spread(
          node,
          except(props, [
            "children",
            "style",
            "class",
            "backdropStyle",
            "backdropClass",
            "backdropTestID",
            "open",
            "animation",
            "onOpen",
            "onClose",
            "onCancel",
            "onOpenChange",
          ]),
        );
        const styledBackdrop = () => props.backdropClass != null || props.backdropStyle != null;
        const slide =
          styledBackdrop() &&
          (props.animation == null || props.animation === "slide") &&
          props.style?.translate == null &&
          !reduceMotion();
        if (slide) {
          rt.prop(node, "animation", "fade");
        } else {
          rt.bindProp(node, "animation", () => props.animation);
        }
        const [insets, onInsets] = nativeInsets(false);
        rt.prop(node, "onInsets", onInsets);
        dom.listen(node, "onShow", () => props.onOpen?.());
        dom.listen(node, "onDismiss", () => props.onClose?.());
        dom.listen(node, "onRequestClose", (e) => {
          const reason =
            e?.reason === "backdropPress" || e?.reason === "swipe" ? e.reason : "cancel";
          close(reason);
        });
        // Tailwind classes are not applied. The fill is a style, so the backdrop
        // (or the default closeOnBackdropPress target) has a frame to press.
        const backdrop = rt.cond(
          () => styledBackdrop() || props.closeOnBackdropPress !== false,
          () => {
            const view = rt.element("view");
            rt.bindProp(view, "class", () =>
              typeof props.backdropClass === "string" ? props.backdropClass : "",
            );
            rt.bindProp(view, "style", () => {
              const fill = { position: "absolute", top: 0, right: 0, bottom: 0, left: 0 };
              return props.backdropStyle == null ? fill : [fill, props.backdropStyle];
            });
            rt.bindProp(view, "testID", () => props.backdropTestID);
            rt.prop(view, "onPress", () => close("backdropPress"));
            return view;
          },
        );
        const popup = rt.element("view");
        rt.bindProp(popup, "class", () => props.class);
        if (slide) {
          const offset = animation.createAnimatedValue(Math.max(1, safeArea().height || 600));
          rt.bindProp(popup, "style", () => [props.style, { translate: [0, offset] }]);
          // timing starts at once and returns a promise. There is no composite `.start()`.
          animation.Animated.timing(offset, {
            toValue: 0,
            duration: 320,
            easing: [0.2, 0.8, 0.2, 1],
          });
        } else {
          rt.bindProp(popup, "style", () => props.style);
        }
        rt.insert(
          popup,
          withInsets(insets, () => props.children),
        );
        rt.insert(node, [backdrop, popup]);
        return node;
      });
    });
  }

  return {
    createNavStack,
    NavStack,
    NavPage,
    Dialog,
    insetsContext,
    NavHeader,
    setNavHeader,
  };
}
