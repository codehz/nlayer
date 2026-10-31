// The app's modules, evaluated by the page. Wasm holds the reactive graph
// and the node table; this file is the facade `@nlayer/runtime`,
// `@nlayer/ui` and `nlayer:rt` share.
import { Values } from "./values.js";
import { createStorage, storagePrefix } from "./storage.js";
import { createNavigation } from "./navigation.js";
import { createLists } from "./list.js";
import { createAnimation } from "./animation.js";

/**
 * `options.appId` names the app's `localStorage` prefix (`nlayer.json`'s
 * id); without one it is the page's directory.
 */
export function createFacade(wasm, requestPump, options = {}) {
  const values = new Values();
  let live = true;
  const handlers = new Map();
  const signalFinalizers = new FinalizationRegistry((id) => {
    if (live) call(114, id);
  });
  const nodeFinalizers = new FinalizationRegistry((id) => {
    if (!live || id <= 1) return;
    values.nodes.delete(id);
    call(307, id);
    requestPump();
  });

  function call(op, ...args) {
    if (!live) throw new Error("the app was disposed");
    const outer = values.depth === 0;
    const value = values.boundary(() =>
      values.with([args], ([id]) => values.result(wasm.nlayer_browser_call(op, id))),
    );
    if (outer) requestPump();
    return value;
  }

  function isNode(value) {
    return values.nodeIds.has(value);
  }

  function node(id, text = false) {
    const existing = values.nodes.get(id)?.deref();
    if (existing) return existing;
    const value = Object.create(null);
    Object.defineProperty(value, "nodeName", { value: text ? "#text" : "nlayer" });
    values.nodeIds.set(value, id);
    values.nodes.set(id, new WeakRef(value));
    if (id !== 1) nodeFinalizers.register(value, id);
    return value;
  }
  values.makeNode = node;
  const root = node(1);

  function accessor(id) {
    const read = () => call(101, id);
    read.set = (next) => {
      call(102, id, next);
    };
    read.update = (fn) => {
      read.set(fn(read()));
    };
    signalFinalizers.register(read, id);
    return read;
  }

  function signal(value, options) {
    return accessor(call(100, value, options));
  }

  function computed(fn, options) {
    return accessor(call(103, fn, options));
  }

  function effect(fn) {
    call(
      104,
      () => {
        const result = fn();
        if (typeof result === "function") call(109, result);
      },
      true,
    );
  }

  function renderEffect(fn) {
    call(104, fn, false);
  }

  function cleanup(fn) {
    call(109, fn);
  }

  function batch(fn) {
    return call(107, fn);
  }

  function untracked(fn) {
    return call(108, fn);
  }

  const boundaryKey = { boundary: true };

  function context(defaultValue) {
    const key = {};
    const read = () => call(112, key, defaultValue);
    read.Provider = (props) => {
      call(113, key, props?.value);
      return props?.children;
    };
    return read;
  }

  // Rust's catch looks up this object. The facade's key is the one the
  // graph was given at startup, swapped in once the bridge can be called.
  function installBoundary() {
    installColorScheme();
    boundaryProvide = call(116);
    safeAreaSignal = signal({ top: 0, right: 0, bottom: 0, left: 0 });
    reduceMotionSignal = signal(false);
    call(400, event);
    call(401, lists.bind);
    call(402, dispatchBack);
  }
  let boundaryProvide = boundaryKey;
  const provideBoundary = (failed) => call(113, boundaryProvide, failed);

  function dynamic(value, depth = 0) {
    if (typeof value === "function" && !isNode(value)) return true;
    if (depth > 64 || !Array.isArray(value)) return false;
    return value.some((item) => dynamic(item, depth + 1));
  }

  function prop(target, name, value) {
    if (name === "children" || name === "ref") return;
    const id = values.nodeIds.get(target);
    if (id === undefined) throw new TypeError("nlayer: not a node");
    if (name.startsWith("on")) {
      let table = handlers.get(id);
      if (!table) handlers.set(id, (table = new Map()));
      if (typeof value === "function") table.set(name, value);
      else table.delete(name);
      call(305, target, name, typeof value === "function");
      return;
    }
    call(303, target, name, value);
  }

  function show(parent, value, shown, marker) {
    const next = call(308, parent, value, shown, marker ?? null);
    shown.length = 0;
    if (Array.isArray(next)) shown.push(...next);
    return shown;
  }

  function insertValue(parent, value, marker) {
    if (dynamic(value)) {
      const shown = [];
      renderEffect(() => show(parent, value, shown, marker));
      return;
    }
    show(parent, value, [], marker);
  }

  function append(parent, child) {
    if (isNode(child)) call(302, parent, child, null);
    else insertValue(parent, child, null);
  }

  function insert(parent, value, marker = null) {
    insertValue(parent, value, marker);
  }

  const rt = {
    element: (tag) => call(300, tag),
    text: (value) => call(301, value),
    append,
    insert,
    prop,
    bindProp: (target, name, get) => renderEffect(() => prop(target, name, get())),
    spread(target, props) {
      renderEffect(() => {
        if (!props || typeof props !== "object") return;
        for (const key of Object.keys(props)) prop(target, key, props[key]);
      });
    },
    component: (Component, props) => untracked(() => Component(props)),
    cond(when, then, otherwise) {
      return () => (when() ? then() : otherwise ? otherwise() : undefined);
    },
    each(list, render, key) {
      const rows = new Map();
      return () => {
        const items = list() ?? [];
        const seen = new Set();
        const out = [];
        for (let index = 0; index < items.length; index++) {
          const item = items[index];
          const id = key === "index" ? index : typeof key === "function" ? key(item, index) : item;
          seen.add(id);
          let row = rows.get(id);
          if (!row) {
            let owner;
            const view = call(105, (handle) => {
              owner = handle;
              const itemSignal = signal(item);
              const indexSignal = signal(index);
              row = { owner, item: itemSignal, index: indexSignal };
              return render(itemSignal, indexSignal);
            });
            row.view = view;
            rows.set(id, row);
          } else {
            row.item.set(item);
            row.index.set(index);
          }
          out.push(row.view);
        }
        for (const [id, row] of rows) {
          if (!seen.has(id)) {
            call(106, row.owner);
            rows.delete(id);
          }
        }
        return out;
      };
    },
    merge(...sources) {
      const out = {};
      for (const source of sources) {
        if (!source || typeof source !== "object") continue;
        for (const key of Object.keys(source)) {
          const desc = Object.getOwnPropertyDescriptor(source, key);
          if (desc) Object.defineProperty(out, key, desc);
        }
      }
      return out;
    },
  };

  function mount(app, props) {
    let owner;
    const shown = [];
    call(105, (handle) => {
      owner = handle;
      const value = untracked(() => app(props && typeof props === "object" ? props : {}));
      if (dynamic(value)) renderEffect(() => show(root, value, shown, null));
      else show(root, value, shown, null);
    });
    return () => {
      call(106, owner);
      show(root, [], shown, null);
    };
  }

  function event(id, name, payload) {
    handlers.get(id)?.get(name)?.(payload);
  }

  const storage = createStorage(
    globalThis.localStorage,
    storagePrefix(options.appId, globalThis.location?.href),
  );

  // The system's color scheme. `options.colorScheme` pins it (`nlayer test`
  // passes one so a run does not depend on the machine's appearance).
  const pinnedScheme =
    options.colorScheme === "dark" ? "dark" : options.colorScheme === "light" ? "light" : null;
  const media = globalThis.matchMedia?.("(prefers-color-scheme: dark)");
  // Made after the browser host is initialized (the signal is its op).
  let colorScheme = null;
  function installColorScheme() {
    if (colorScheme) return;
    colorScheme = signal(pinnedScheme ?? (media?.matches ? "dark" : "light"));
    applyRootScheme();
  }
  function applyRootScheme() {
    const doc = globalThis.document;
    if (!colorScheme || !doc?.documentElement) return;
    doc.documentElement.style.colorScheme = colorScheme();
  }
  const onScheme = (event) => {
    if (pinnedScheme || !colorScheme) return;
    colorScheme.set(event.matches ? "dark" : "light");
    applyRootScheme();
  };
  media?.addEventListener?.("change", onScheme);

  // Player owns popstate so navigation gets first refusal. The engine calls
  // this only after Core reports that no modal or stack consumed back.
  const backListeners = [];
  function dispatchBack() {
    for (let i = backListeners.length - 1; i >= 0; i--) {
      if (backListeners[i].handler()) return true;
    }
    return false;
  }
  const BackHandler = {
    addEventListener(handler) {
      if (typeof handler !== "function") {
        throw new TypeError("BackHandler.addEventListener: the handler is not a function");
      }
      const entry = { handler };
      backListeners.push(entry);
      let removed = false;
      return () => {
        if (removed) return;
        removed = true;
        const i = backListeners.indexOf(entry);
        if (i >= 0) backListeners.splice(i, 1);
      };
    },
  };

  // `resource(source, fetcher)` on the facade's signals and effects: the
  // same states and cancellation as the QuickJS host's class.
  function resource(source, fetcher) {
    let readSource = null;
    if (typeof fetcher === "function") {
      readSource = source;
    } else {
      fetcher = source;
    }
    const value = signal(undefined);
    const error = signal(undefined);
    const state = signal("unresolved");
    const trigger = signal(0);
    let current = 0;
    let running = false;
    let hasValue = false;
    let controller = null;
    let refetching = false;

    function stop() {
      current++;
      running = false;
      if (controller) {
        const aborted = controller;
        controller = null;
        aborted.abort();
      }
    }

    function settle(run, ok, outcome) {
      if (!running || run !== current) return;
      running = false;
      controller = null;
      batch(() => {
        if (ok) {
          value.set(outcome);
          error.set(undefined);
          hasValue = true;
          state.set("ready");
        } else {
          error.set(outcome);
          state.set("errored");
        }
      });
    }

    function load() {
      const tick = trigger();
      void tick;
      const src = readSource ? readSource() : undefined;
      stop();
      if (readSource && !src) {
        state.set(hasValue ? "ready" : "unresolved");
        return;
      }
      const run = current;
      running = true;
      state.set(hasValue ? "refreshing" : "pending");
      const wasRefetching = refetching;
      refetching = false;
      controller = new AbortController();
      const info = { signal: controller.signal, refetching: wasRefetching };
      let result;
      try {
        result = untracked(() => (readSource ? fetcher(src, info) : fetcher(info)));
      } catch (failure) {
        settle(run, false, failure);
        return;
      }
      Promise.resolve(result).then(
        (next) => settle(run, true, next),
        (failure) => settle(run, false, failure),
      );
    }

    effect(() => {
      load();
      return stop;
    });

    const read = () => value();
    read.state = () => state();
    read.error = () => error();
    read.loading = () => state() === "pending" || state() === "refreshing";
    read.refetch = () => {
      refetching = true;
      trigger.set(trigger() + 1);
    };
    read.mutate = (next) => {
      hasValue = true;
      if (!running) state.set("ready");
      batch(() => {
        value.set(next);
        error.set(undefined);
      });
    };
    return read;
  }

  const animation = createAnimation();
  const lists = createLists({
    invoke: call,
    values,
    reactive: {
      signal: (value) => signal(value),
      computed,
      batch,
      untracked,
      cleanup,
      renderEffect,
    },
    rt,
    lists: new Map(),
    wake: requestPump,
  });
  let safeAreaSignal;
  let reduceMotionSignal;
  const navigation = createNavigation(
    { signal, computed, untracked, batch, context },
    {
      rt,
      listen: (node, event, handler) => {
        const id = values.nodeIds.get(node);
        if (id === undefined) return;
        let table = handlers.get(id);
        if (!table) handlers.set(id, (table = new Map()));
        table.set(event, handler);
      },
    },
    animation.api,
    () => safeAreaSignal(),
    () => reduceMotionSignal(),
  );

  const runtime = {
    signal,
    computed,
    effect,
    batch,
    untracked,
    cleanup,
    context,
    mount,
    ErrorBoundary,
    resource,
    storage,
  };
  const ui = {
    View: (props) => host("view", props),
    Text: (props) => host("text", props),
    Image: (props) => host("image", props),
    Scroll: (props) => host("scroll", props),
    Input: (props) => host("input", props),
    List: (props) => lists.render(props),
    createAnimatedValue: animation.api.createAnimatedValue,
    Animated: animation.api.Animated,
    createNavStack: navigation.createNavStack,
    NavStack: navigation.NavStack,
    NavPage: navigation.NavPage,
    Dialog: navigation.Dialog,
    platform: {
      os: "web",
      colorScheme: () => {
        installColorScheme();
        return colorScheme();
      },
    },
    BackHandler,
    Keyboard: {
      dismiss() {
        const active = globalThis.document?.activeElement;
        if (active && active.classList?.contains("nl-field")) active.blur();
      },
    },
    env: (name) => {
      if (name === "insets") return navigation.insetsContext();
      const insets = safeAreaSignal();
      if (name === "safe-area-inset-top") return insets.top;
      if (name === "safe-area-inset-right") return insets.right;
      if (name === "safe-area-inset-bottom") return insets.bottom;
      if (name === "safe-area-inset-left") return insets.left;
      if (name === "keyboard-inset-height") return 0;
      return undefined;
    },
    setSafeArea(insets) {
      safeAreaSignal.set(insets);
    },
    setReduceMotion(reduce) {
      reduceMotionSignal.set(!!reduce);
    },
  };
  function host(tag, props) {
    return untracked(() => {
      const el = rt.element(tag);
      const children = props?.children;
      const rest = { ...props };
      delete rest.children;
      rt.spread(el, rest);
      if (children !== undefined) rt.append(el, children);
      return el;
    });
  }

  // The graph's boundary key, not the facade's placeholder.
  function ErrorBoundary(props) {
    const failed = signal(undefined);
    return computed(() => {
      const state = failed();
      if (Array.isArray(state)) {
        const fallback = props?.fallback;
        if (typeof fallback === "function" && fallback.length > 0) {
          return untracked(() => fallback(state[0], () => failed.set(undefined)));
        }
        return fallback;
      }
      provideBoundary(failed);
      const children = props?.children;
      return typeof children === "function" ? children() : children;
    });
  }

  return {
    values,
    imports: values.imports(() => wasm.memory),
    runtime,
    ui,
    rt,
    installBoundary,
    animation,
    event,
    dispatchBack,
    dispose() {
      live = false;
      values.closed = true;
      media?.removeEventListener?.("change", onScheme);
      animation.dispose();
    },
  };
}

/** Evaluates `source` (an ES module) in a fresh realm whose import map
 * points the native modules at `namespaces`. */
export function loadBundle(source, namespaces) {
  const frame = document.createElement("iframe");
  frame.hidden = true;
  frame.setAttribute("aria-hidden", "true");
  document.documentElement.append(frame);
  const doc = frame.contentDocument;
  const realm = frame.contentWindow;
  realm.__nlayer = namespaces;
  const imports = {};
  const urls = [];
  for (const [specifier, namespace] of Object.entries(namespaces)) {
    const body = Object.keys(namespace)
      .map((name, index) => {
        const spec = JSON.stringify(specifier);
        const key = JSON.stringify(name);
        return `const e${index} = globalThis.__nlayer[${spec}][${key}]; export { e${index} as ${name} };`;
      })
      .join("\n");
    const url = URL.createObjectURL(new Blob([body], { type: "text/javascript" }));
    urls.push(url);
    imports[specifier] = url;
  }
  const map = doc.createElement("script");
  map.type = "importmap";
  map.textContent = JSON.stringify({ imports });
  doc.head.append(map);
  const app = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  urls.push(app);
  return new Promise((resolve, reject) => {
    realm.__nlayerBundle = (error) => {
      if (error) reject(new Error(String(error)));
      else {
        resolve(() => {
          frame.remove();
          for (const url of urls) URL.revokeObjectURL(url);
        });
      }
    };
    const script = doc.createElement("script");
    script.type = "module";
    // An inline module's `load` event does not fire in headless Chrome, so
    // the module reports completion itself.
    script.textContent = `import(${JSON.stringify(app)}).then(() => globalThis.__nlayerBundle(), (error) => globalThis.__nlayerBundle(String(error && (error.stack || error.message) || error)));`;
    doc.head.append(script);
  });
}
