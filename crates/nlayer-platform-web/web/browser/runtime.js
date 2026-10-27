// The app's modules, evaluated by the page. Wasm holds the reactive graph
// and the node table; this file is the facade `@nlayer/runtime`,
// `@nlayer/ui` and `nlayer:rt` share.
import { Values } from "./values.js";

export function createFacade(wasm, requestPump) {
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
    call(104, () => {
      const result = fn();
      if (typeof result === "function") call(109, result);
    }, true);
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
      call(113, boundaryKey, failed);
      const children = props?.children;
      return typeof children === "function" ? children() : children;
    });
  }

  // Rust's catch looks up this object. The facade's key is the one the
  // graph was given at startup, swapped in once the bridge can be called.
  function installBoundary() {
    const key = call(116);
    // The graph compares by identity. Replace the placeholder's identity
    // by providing through the real key from here on.
    boundaryProvide = call(116);
    call(400, event);
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
  };
  const ui = {
    View: (props) => host("view", props),
    Text: (props) => host("text", props),
    Image: (props) => host("image", props),
    Scroll: (props) => host("scroll", props),
    Input: (props) => host("input", props),
    platform: { os: "web" },
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

  // ErrorBoundary provides the graph's key, not the placeholder.
  const realBoundary = ErrorBoundary;
  function boundary(props) {
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
  runtime.ErrorBoundary = boundary;
  void realBoundary;
  void installBoundary;

  return {
    values,
    imports: values.imports(() => wasm.memory),
    runtime,
    ui,
    rt,
    event,
    installBoundary,
    dispose() {
      live = false;
      values.closed = true;
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
    const script = doc.createElement("script");
    script.type = "module";
    script.textContent = `import ${JSON.stringify(app)};`;
    script.onload = () => resolve(() => {
      frame.remove();
      for (const url of urls) URL.revokeObjectURL(url);
    });
    script.onerror = () => reject(new Error("could not load the app module"));
    doc.head.append(script);
  });
}
