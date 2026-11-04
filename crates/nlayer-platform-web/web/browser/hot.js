// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// Hot module replacement on the browser host (development bundles only):
// the module system and component registry behind `__nlayer_hot`, as
// `nlayer-js` `hot.rs` is for QuickJS, with the same semantics.
//
// A development bundle defines every module as a factory and starts the
// entry with the namespaces of the native modules it imports. An update
// script redefines the modules that changed; `update` runs them again,
// together with the importers that cannot take the change on their own,
// in place. `import.meta.hot` is `meta(id)`.
//
// `update` returns false when a change cannot reach an importer or the
// entry, and when a module declines or an accept/dispose/prune callback
// throws. Only the latter sets `reload` (read with `takeReload`). A
// factory that throws is reported by the caller and is not a reload.

const RELOAD = Symbol("reload");

// `reactive` is the facade's graph (signals, computed, untracked, batch)
// and `report` shows an error the app would otherwise lose.
export function createHot(reactive, report) {
  const factories = new Map();
  const fixed = new Set();
  const modules = new Map();
  const importers = new Map();
  const components = new Map();
  const proxies = new WeakSet();
  const contexts = new Map();
  const kept = new Map();
  const undo = new Map();
  const renders = new Map();
  const pending = new Map();
  const metas = new Map();
  let natives = {};
  let entry = null;
  let current = null;
  let forced = false;
  let updating = false;
  let stale = new Set();
  let applied = true;
  let reload = false;

  const keyOf = (module, name) => `${module}:${name}`;
  const sorted = (values) => [...values].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const undoRenders = (list) => {
    for (const render of list) {
      try {
        render.dispose();
      } catch (error) {
        report(error);
      }
    }
  };
  const bump = (version) => {
    if (typeof version.update === "function") version.update((n) => n + 1);
    else version.set((version() || 0) + 1);
  };
  const requestReload = () => {
    reload = true;
  };

  // `spec` relative to module `from` (`a/b.js` + `../c.js` -> `c.js`).
  const resolve = (from, spec) => {
    const parts = from.split("/");
    parts.pop();
    for (const part of spec.split("/")) {
      if (part === "..") parts.pop();
      else if (part !== ".") parts.push(part);
    }
    return parts.join("/");
  };
  const resolveDep = (from, spec) =>
    String(spec).startsWith(".") ? resolve(from, spec) : String(spec);

  const require = (from, spec) => {
    spec = String(spec);
    if (!spec.startsWith(".")) {
      const namespace = natives?.[spec];
      if (namespace === undefined) throw new TypeError(`nlayer hot: no module ${spec}`);
      return namespace;
    }
    const id = resolve(from, spec);
    let set = importers.get(id);
    if (!set) importers.set(id, (set = new Set()));
    set.add(from);
    const loaded = load(id);
    if (loaded === RELOAD) {
      requestReload();
      throw new TypeError("nlayer hot: the module declined to run");
    }
    return loaded;
  };

  const forgetImports = (id) => {
    for (const set of importers.values()) set.delete(id);
  };

  const load = (id) => {
    const known = modules.get(id);
    if (known) return known.exports;
    const exports = {};
    modules.set(id, { exports, components: 0, accepts: false, ran: false });
    if (run(id) === RELOAD) return RELOAD;
    return exports;
  };

  const forward = (keptExports, exports) => {
    const keys = Object.getOwnPropertyNames(exports);
    for (const key of Object.getOwnPropertyNames(keptExports)) {
      if (!keys.includes(key)) delete keptExports[key];
    }
    for (const key of keys) {
      const { enumerable } = Object.getOwnPropertyDescriptor(exports, key);
      Object.defineProperty(keptExports, key, {
        configurable: true,
        enumerable,
        get: () => exports[key],
      });
    }
  };

  // Whether `module` takes its own updates: it made components and
  // everything it exports (enumerably) is one of their proxies.
  const accepts = (module, exports) => {
    if (!module.components) return false;
    return Object.keys(exports).every((key) => proxies.has(exports[key]));
  };

  const run = (id) => {
    const factory = factories.get(id);
    const module = modules.get(id);
    if (!factory || !module) throw new TypeError(`nlayer hot: no module ${id}`);
    if (fixed.has(id) && module.ran) return undefined;
    module.components = 0;
    pending.set(id, renders.get(id) ?? []);
    renders.delete(id);
    forgetImports(id);
    const meta = metas.get(id);
    const dispose = meta ? meta.dispose.splice(0) : [];
    const selfCb = meta?.selfCb ?? null;
    const undone = undo.get(id) ?? [];
    undo.delete(id);
    if (meta) {
      meta.selfCb = null;
      meta.depAccepts.length = 0;
      meta.prune.length = 0;
      meta.selfAccept = false;
    }
    const data = meta?.data;
    for (const cb of dispose) {
      try {
        cb(data);
      } catch (error) {
        report(error);
        requestReload();
        return RELOAD;
      }
    }
    for (const fn of undone) {
      try {
        fn();
      } catch (error) {
        report(error);
      }
    }
    const exports = fixed.has(id) ? module.exports : {};
    const record = { exports };
    const previous = current;
    current = id;
    let failure = null;
    try {
      factory(record, exports, (spec) => require(id, spec));
    } catch (error) {
      failure = error;
    } finally {
      current = previous;
    }
    const left = pending.get(id) ?? [];
    pending.delete(id);
    undoRenders(left);
    if (failure) throw failure;
    if (record.exports !== module.exports) forward(module.exports, record.exports);
    module.accepts = accepts(module, module.exports);
    module.ran = true;
    if (selfCb && !metas.get(id)?.invalidated) {
      try {
        selfCb(module.exports);
      } catch (error) {
        report(error);
        requestReload();
        return RELOAD;
      }
    }
    if (reload) return RELOAD;
    return undefined;
  };

  const stops = (id) => {
    if (id === entry) return true;
    const meta = metas.get(id);
    if (meta?.invalidated) return false;
    return !!(modules.get(id)?.accepts || meta?.selfAccept);
  };

  const acceptsDep = (importer, dep) => {
    const meta = metas.get(importer);
    if (!meta) return false;
    return meta.depAccepts.some((accept) => accept.deps.includes(dep));
  };

  // False when `id` has no importer to take the change.
  const pushImporters = (id, queue) => {
    const from = importers.get(id);
    if (!from?.size) return false;
    for (const importer of sorted(from)) {
      if (fixed.has(importer)) continue;
      if (acceptsDep(importer, id)) continue;
      queue.push(importer);
    }
    return true;
  };

  const topo = (rerun) => {
    const seen = new Set();
    const order = [];
    const visit = (id) => {
      if (seen.has(id)) return;
      seen.add(id);
      for (const dep of sorted(importers.keys())) {
        if (rerun.has(dep) && importers.get(dep).has(id)) visit(dep);
      }
      order.push(id);
    };
    for (const id of sorted(rerun)) visit(id);
    return order;
  };

  // Modules to run again for `seeds`, dependencies before importers.
  // `already` propagates (an `invalidate`) but is not run again. Null when
  // a change reaches a module nothing imports and that is not the entry.
  const plan = (seeds, already) => {
    const rerun = new Set();
    const seen = new Set();
    const consumed = [];
    const queue = [...seeds];
    while (queue.length) {
      const id = queue.pop();
      if (fixed.has(id) || seen.has(id)) continue;
      seen.add(id);
      if (metas.get(id)?.invalidated) consumed.push(id);
      if (already.has(id)) {
        if (!pushImporters(id, queue)) return null;
        continue;
      }
      rerun.add(id);
      if (stops(id)) continue;
      if (!pushImporters(id, queue)) return null;
    }
    return { order: topo(rerun), consumed };
  };

  const clearInvalidated = (ids) => {
    for (const id of ids) {
      const meta = metas.get(id);
      if (meta) meta.invalidated = false;
    }
  };

  const takeInvalidated = (ran) => {
    const ids = [];
    for (const [id, meta] of metas) {
      if (meta.invalidated && ran.has(id)) {
        meta.invalidated = false;
        ids.push(id);
      }
    }
    return ids;
  };

  const depCallbacks = (ran) => {
    const out = [];
    for (const id of sorted(metas.keys())) {
      if (ran.has(id)) continue;
      const meta = metas.get(id);
      meta.depAccepts.forEach((accept, index) => {
        if (accept.deps.some((dep) => ran.has(dep))) out.push([id, index]);
      });
    }
    return out;
  };

  const namespaceOf = (id) => modules.get(id)?.exports;

  const fireDepCallbacks = (callbacks) => {
    for (const [id, index] of callbacks) {
      const accept = metas.get(id)?.depAccepts[index];
      if (!accept) continue;
      const arg = accept.array
        ? accept.deps.map((dep) => namespaceOf(dep))
        : namespaceOf(accept.deps[0] ?? "");
      try {
        accept.cb(arg);
      } catch (error) {
        report(error);
        requestReload();
        return false;
      }
    }
    return true;
  };

  const reachable = () => {
    if (entry == null) return new Set();
    const seen = new Set();
    const stack = [entry];
    while (stack.length) {
      const id = stack.pop();
      if (seen.has(id)) continue;
      seen.add(id);
      for (const [dep, users] of importers) {
        if (users.has(id)) stack.push(dep);
      }
    }
    return seen;
  };

  const pruneUnreachable = () => {
    for (;;) {
      const live = reachable();
      let leaving = null;
      for (const id of modules.keys()) {
        if (id !== entry && !live.has(id) && !fixed.has(id)) {
          leaving = id;
          break;
        }
      }
      if (leaving == null) return true;
      importers.delete(leaving);
      for (const set of importers.values()) set.delete(leaving);
      const meta = metas.get(leaving);
      let pruneCbs = [];
      let data;
      if (meta) {
        meta.selfAccept = false;
        meta.depAccepts.length = 0;
        meta.dispose.length = 0;
        pruneCbs = meta.prune.splice(0);
        meta.selfCb = null;
        data = meta.data;
      }
      modules.delete(leaving);
      const mounted = renders.get(leaving) ?? [];
      renders.delete(leaving);
      pending.delete(leaving);
      undoRenders(mounted);
      const undoFns = undo.get(leaving) ?? [];
      undo.delete(leaving);
      for (const fn of undoFns) {
        try {
          fn();
        } catch (error) {
          report(error);
        }
      }
      for (const cb of pruneCbs) {
        try {
          cb(data);
        } catch (error) {
          report(error);
          requestReload();
          return false;
        }
      }
    }
  };

  const rerunAll = (order, defined, ran) => {
    for (const id of order) {
      if (metas.get(id)?.declined) {
        requestReload();
        return false;
      }
      forced = !defined.has(id);
      if (run(id) === RELOAD) return false;
      ran.add(id);
      if (reload) return false;
    }
    return true;
  };

  // A proxy's render: a memo of the component's implementation now, run
  // again when its version changes.
  const render = (key, version, props) =>
    reactive.computed(() => {
      version();
      const fn = components.get(key)?.fn;
      if (!fn) return undefined;
      try {
        return reactive.untracked(() => fn(props));
      } catch (error) {
        report(error);
        return undefined;
      }
    });

  const ensureMeta = (id) => {
    let meta = metas.get(id);
    if (meta) return meta;
    const data = {};
    const object = {
      data,
      accept(first, second) {
        const state = metas.get(id);
        if (!state) return;
        if (arguments.length === 0 || first === undefined) {
          state.selfAccept = true;
          state.selfCb = null;
          return;
        }
        if (arguments.length === 1 && typeof first === "function") {
          state.selfAccept = true;
          state.selfCb = first;
          return;
        }
        if (typeof second !== "function") {
          throw new TypeError("nlayer hot: accept expected a callback");
        }
        const list = Array.isArray(first) ? first.map(String) : [String(first)];
        state.depAccepts.push({
          deps: list.map((dep) => resolveDep(id, dep)),
          array: Array.isArray(first),
          cb: second,
        });
      },
      dispose(cb) {
        pushCallback(id, cb, true);
      },
      prune(cb) {
        pushCallback(id, cb, false);
      },
      invalidate() {
        const state = metas.get(id);
        if (state) state.invalidated = true;
      },
      decline() {
        const state = metas.get(id);
        if (state) state.declined = true;
        requestReload();
      },
    };
    meta = {
      object,
      data,
      selfAccept: false,
      selfCb: null,
      depAccepts: [],
      dispose: [],
      prune: [],
      invalidated: false,
      declined: false,
    };
    metas.set(id, meta);
    return meta;
  };

  const pushCallback = (id, cb, dispose) => {
    if (typeof cb !== "function") throw new TypeError("nlayer hot: expected a function");
    const state = metas.get(id);
    if (!state) return;
    if (dispose) state.dispose.push(cb);
    else state.prune.push(cb);
  };

  const hot = {
    define(id, factory, isFixed) {
      id = String(id);
      factories.set(id, factory);
      if (isFixed) fixed.add(id);
    },
    start(id, namespaces) {
      entry = String(id);
      natives = namespaces ?? {};
      if (load(entry) === RELOAD) return false;
      return true;
    },
    update(defs) {
      applied = false;
      if (reload) return false;
      const changed = [];
      const defined = new Set();
      for (const [id, factory] of Object.entries(defs ?? {})) {
        if (modules.has(id) && !fixed.has(id)) changed.push(id);
        factories.set(id, factory);
        defined.add(id);
      }
      updating = true;
      stale = new Set();
      try {
        const planned = plan(changed, new Set());
        if (!planned) return false;
        clearInvalidated(planned.consumed);
        applied = true;
        const ran = new Set();
        if (!rerunAll(planned.order, defined, ran)) {
          applied = false;
          return false;
        }
        for (;;) {
          const invalidated = takeInvalidated(ran);
          if (invalidated.length) {
            const extra = plan(invalidated, ran);
            if (!extra) {
              applied = false;
              return false;
            }
            clearInvalidated(extra.consumed);
            if (!rerunAll(extra.order, defined, ran)) {
              applied = false;
              return false;
            }
            continue;
          }
          const callbacks = depCallbacks(ran);
          if (!callbacks.length) break;
          forced = false;
          for (const [id] of callbacks) ran.add(id);
          if (!fireDepCallbacks(callbacks) || reload) {
            applied = false;
            return false;
          }
        }
        if (!pruneUnreachable() || reload) {
          applied = false;
          return false;
        }
        const versions = [...stale].map((key) => components.get(key)?.version).filter(Boolean);
        stale = new Set();
        reactive.batch(() => {
          for (const version of versions) bump(version);
        });
        return true;
      } finally {
        updating = false;
        forced = false;
      }
    },
    component(module, name, fn, sig) {
      const key = keyOf(module, name);
      sig = String(sig);
      const owner = current && modules.get(current);
      if (owner) owner.components++;
      const existing = components.get(key);
      if (existing) {
        if (existing.sig !== sig || forced) {
          if (updating) stale.add(key);
          else reactive.batch(() => bump(existing.version));
        }
        existing.fn = fn;
        existing.sig = sig;
        return existing.proxy;
      }
      const version = reactive.signal(0);
      const proxy = (props) => render(key, version, props);
      Object.defineProperty(proxy, "name", { value: String(name), configurable: true });
      proxies.add(proxy);
      components.set(key, { fn, sig, version, proxy });
      return proxy;
    },
    context(module, name, made) {
      const key = keyOf(module, name);
      const existing = contexts.get(key);
      if (existing) {
        reactive.adoptContextDefault?.(existing, made);
        return existing;
      }
      contexts.set(key, made);
      return made;
    },
    keep(module, name, sig, value) {
      const key = keyOf(module, name);
      sig = String(sig);
      const existing = kept.get(key);
      if (existing && existing.sig === sig) return existing.value;
      kept.set(key, { sig, value });
      return value;
    },
    meta(id) {
      return ensureMeta(String(id)).object;
    },
  };

  return {
    hot,
    get started() {
      return entry !== null;
    },
    // True once, then cleared. Set by `decline()` or a throwing
    // accept/dispose/prune callback. A bubble that stops short does not.
    takeReload() {
      const value = reload;
      reload = false;
      return value;
    },
    async apply(load) {
      applied = true;
      try {
        await load();
      } catch (error) {
        report(error);
      }
      return applied && !reload;
    },
    rendered(app, dispose) {
      if (current === null) return;
      let list = renders.get(current);
      if (!list) renders.set(current, (list = []));
      list.push({ app, dispose });
    },
    reuse(app, props) {
      if (current === null) return null;
      const list = pending.get(current);
      if (!list) return null;
      const hasProps = props !== null && (typeof props === "object" || typeof props === "function");
      if (!hasProps && list.length && list[0].app === app) {
        const keptRender = list.shift();
        let done = renders.get(current);
        if (!done) renders.set(current, (done = []));
        done.push(keptRender);
        return keptRender.dispose;
      }
      undoRenders(list.splice(0));
      return null;
    },
    undoOnRerun(fn) {
      if (current === null) return false;
      let list = undo.get(current);
      if (!list) undo.set(current, (list = []));
      list.push(fn);
      return true;
    },
    forgetUndo(fn) {
      for (const list of undo.values()) {
        const at = list.indexOf(fn);
        if (at >= 0) {
          list.splice(at, 1);
          return;
        }
      }
    },
  };
}
