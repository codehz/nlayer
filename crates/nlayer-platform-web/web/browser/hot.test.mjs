// The browser host's hot module replacement runtime without a browser or
// nlayer.wasm: `node --test crates/nlayer-platform-web/web/browser/hot.test.mjs`.
// A small reactive graph stands in for the engine's.
import assert from "node:assert/strict";
import test from "node:test";
import { createHot } from "./hot.js";

function fakeReactive() {
  let tracking = null;
  const signal = (value) => {
    const read = () => {
      tracking?.add(read);
      return value;
    };
    read.version = 0;
    read.set = (next) => {
      value = next;
      read.version++;
    };
    read.update = (fn) => read.set(fn(untracked(read)));
    return read;
  };
  const untracked = (fn) => {
    const outer = tracking;
    tracking = null;
    try {
      return fn();
    } finally {
      tracking = outer;
    }
  };
  const computed = (fn) => {
    let deps = null;
    let value;
    return () => {
      if (!deps || [...deps].some(([dep, version]) => dep.version !== version)) {
        const outer = tracking;
        const seen = (tracking = new Set());
        try {
          value = fn();
        } finally {
          tracking = outer;
        }
        deps = new Map([...seen].map((dep) => [dep, dep.version]));
      }
      return value;
    };
  };
  const defaults = new WeakMap();
  return {
    signal,
    computed,
    untracked,
    batch: (fn) => fn(),
    context(defaultValue) {
      const box = { value: defaultValue };
      const read = () => box.value;
      defaults.set(read, box);
      return read;
    },
    adoptContextDefault(kept, from) {
      const target = defaults.get(kept);
      const source = defaults.get(from);
      if (target && source && target !== source) target.value = source.value;
    },
  };
}

function setup() {
  const errors = [];
  const reactive = fakeReactive();
  const runtime = createHot(reactive, (error) => errors.push(error));
  return { runtime, hot: runtime.hot, errors, reactive };
}

function fakeMount(runtime) {
  const mounted = [];
  const mount = (App, props) => {
    const kept = runtime.reuse(App, props);
    if (kept) return kept;
    const entry = { App, out: App(props ?? {}) };
    mounted.push(entry);
    const dispose = () => {
      const at = mounted.indexOf(entry);
      if (at >= 0) mounted.splice(at, 1);
    };
    runtime.rendered(App, dispose);
    return dispose;
  };
  return { mount, mounted };
}

test("start runs dependencies first; an update reruns the changed module and its importers, dependencies first", () => {
  const { hot } = setup();
  const log = [];
  const natives = { "nlayer:rt": { name: "native" } };
  let native;
  let lib;
  hot.define("util.js", (module, exports) => {
    log.push("util 1");
    exports.value = 1;
  });
  hot.define("lib.js", (module, exports, require) => {
    log.push("lib");
    const util = require("./util.js");
    exports.double = () => util.value * 2;
  });
  hot.define("App.js", (module, exports, require) => {
    log.push("App");
    native = require("nlayer:rt").name;
    lib = require("./lib.js");
  });
  hot.start("App.js", natives);
  assert.deepEqual(log, ["App", "lib", "util 1"]);
  log.length = 0;
  assert.equal(
    hot.update({
      "util.js": (module, exports) => {
        log.push("util 2");
        exports.value = 5;
      },
    }),
    true,
  );
  assert.deepEqual(log, ["util 2", "lib", "App"]);
  assert.equal(native, "native");
  assert.equal(lib.double(), 10);
});

test("an importer keeps the exports object: it forwards to the latest run's exports", () => {
  const { hot } = setup();
  let held;
  hot.define("data.js", (module, exports) => {
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.a = 1;
    exports.b = 2;
  });
  hot.define("App.js", (module, exports, require) => {
    held ??= require("./data.js");
  });
  hot.start("App.js", {});
  assert.equal(held.a, 1);
  hot.update({
    "data.js": (module, exports) => {
      exports.a = 10;
      exports.c = 3;
    },
  });
  assert.equal(held.a, 10);
  assert.equal(held.c, 3);
  assert.equal("b" in held, false);
  assert.deepEqual(Object.keys(held), ["a", "c"]);
});

test("a component module takes its own updates; its proxy stays and renders the new code", () => {
  const { hot, errors } = setup();
  let appRuns = 0;
  let Button;
  const define = (label, sig) => (module, exports) => {
    exports.Button = hot.component("Button.js", "Button", (props) => `${label} ${props.n}`, sig);
  };
  hot.define("Button.js", define("one", "s1"));
  hot.define("App.js", (module, exports, require) => {
    appRuns++;
    Button = require("./Button.js").Button;
  });
  hot.start("App.js", {});
  const proxy = Button;
  const view = proxy({ n: 1 });
  assert.equal(proxy.name, "Button");
  assert.equal(view(), "one 1");
  assert.equal(hot.update({ "Button.js": define("two", "s2") }), true);
  assert.equal(appRuns, 1);
  assert.equal(Button, proxy);
  assert.equal(view(), "two 1");
  hot.update({ "Button.js": define("three", "s2") });
  assert.equal(view(), "two 1");
  hot.update({
    "Button.js": (module, exports) => {
      exports.Button = hot.component(
        "Button.js",
        "Button",
        () => {
          throw new Error("broken");
        },
        "s3",
      );
    },
  });
  assert.equal(view(), undefined);
  assert.equal(errors[0].message, "broken");
  hot.update({ "Button.js": define("fixed", "s4") });
  assert.equal(view(), "fixed 1");
});

test("components of a module run again for a dependency render again with the same code", () => {
  const { hot } = setup();
  let View;
  hot.define("theme.js", (module, exports) => {
    exports.color = "red";
  });
  hot.define("App.js", (module, exports, require) => {
    const theme = require("./theme.js");
    View = exports.View = hot.component("App.js", "View", () => theme.color, "same");
  });
  hot.start("App.js", {});
  const view = View({});
  assert.equal(view(), "red");
  hot.update({
    "theme.js": (module, exports) => {
      exports.color = "blue";
    },
  });
  assert.equal(view(), "blue");
});

test("keep returns the first value while the initializer's signature stays", () => {
  const { hot } = setup();
  const values = [];
  const make = (sig) => (module, exports) => {
    values.push(hot.keep("App.js", "count", sig, { made: values.length }));
  };
  hot.define("App.js", make("a"));
  hot.start("App.js", {});
  hot.update({ "App.js": make("a") });
  hot.update({ "App.js": make("b") });
  assert.equal(values[1], values[0]);
  assert.notEqual(values[2], values[0]);
  assert.equal(values[2].made, 2);
});

test("a context keeps its identity across runs and takes the new default", () => {
  const { hot, reactive } = setup();
  const contexts = [];
  const make = (theme) => (module, exports) => {
    contexts.push(hot.context("App.js", "Theme", reactive.context(theme)));
  };
  hot.define("App.js", make("light"));
  hot.start("App.js", {});
  assert.equal(contexts[0](), "light");
  hot.update({ "App.js": make("dark") });
  assert.equal(contexts[1], contexts[0]);
  assert.equal(contexts[0](), "dark");
});

test("a run keeps a mount of the same component and undoes the mounts it does not make again", () => {
  const { runtime, hot } = setup();
  const { mount, mounted } = fakeMount(runtime);
  const app = (sig) => (module, exports) => {
    const Root = hot.component("App.js", "mount#0", () => "root", sig);
    mount(Root);
  };
  hot.define("App.js", app("a"));
  hot.start("App.js", {});
  assert.equal(mounted.length, 1);
  const first = mounted[0];
  hot.update({ "App.js": app("b") });
  assert.deepEqual(mounted, [first]);
  hot.update({ "App.js": () => {} });
  assert.deepEqual(mounted, []);
  hot.update({ "App.js": app("c") });
  hot.update({
    "App.js": (module, exports) => {
      mount(() => "other");
    },
  });
  assert.equal(mounted.length, 1);
  assert.equal(mounted[0].out, "other");
  hot.update({
    "App.js": (module, exports) => {
      const Root = hot.component("App.js", "mount#0", () => "root", "c");
      mount(Root, { a: 1 });
    },
  });
  assert.equal(mounted.length, 1);
  assert.equal(mounted[0].out(), "root");
});

test("top-level undo runs before the module runs again, and forgetUndo drops it", () => {
  const { runtime, hot } = setup();
  const log = [];
  const app = (label, forget) => () => {
    const undo = () => log.push(`undo ${label}`);
    assert.equal(runtime.undoOnRerun(undo), true);
    log.push(label);
    if (forget) {
      undo();
      runtime.forgetUndo(undo);
    }
  };
  hot.define("App.js", app("mod+k"));
  hot.start("App.js", {});
  hot.update({ "App.js": app("mod+j") });
  assert.deepEqual(log, ["mod+k", "undo mod+k", "mod+j"]);
  hot.update({ "App.js": app("mod+l", true) });
  const sent = log.length;
  hot.update({ "App.js": () => {} });
  assert.equal(log.length, sent);
  assert.equal(
    runtime.undoOnRerun(() => {}),
    false,
  );
});

test("apply reports an update that throws, and reports whether update applied", async () => {
  const { runtime, hot, errors } = setup();
  hot.define("App.js", (module, exports, require) => {
    require("./a.js");
  });
  hot.define("a.js", () => {});
  hot.start("App.js", {});
  assert.equal(runtime.started, true);
  assert.equal(
    await runtime.apply(async () => {
      hot.update({
        "a.js": () => {
          throw new Error("oops");
        },
      });
    }),
    true,
  );
  assert.equal(errors[0].message, "oops");
  assert.equal(await runtime.apply(async () => {}), true);
});

test("accept keeps the importer from rerunning", () => {
  const { hot } = setup();
  let midRuns = 0;
  let appRuns = 0;
  hot.define("mid.js", (module, exports) => {
    midRuns++;
    hot.meta("mid.js").accept();
    exports.value = midRuns;
  });
  hot.define("app.js", (module, exports, require) => {
    appRuns++;
    exports.value = require("./mid.js").value;
  });
  hot.start("app.js", {});
  assert.equal(
    hot.update({
      "mid.js": (module, exports) => {
        midRuns++;
        hot.meta("mid.js").accept();
        exports.value = midRuns;
      },
    }),
    true,
  );
  assert.equal(midRuns, 2);
  assert.equal(appRuns, 1);
});

test("accept(dep) does not rerun the accepter and calls back with the new namespace", () => {
  const { hot } = setup();
  const seen = [];
  let midRuns = 0;
  let appRuns = 0;
  hot.define("mid.js", (module, exports) => {
    midRuns++;
    exports.value = midRuns;
  });
  hot.define("app.js", (module, exports, require) => {
    appRuns++;
    hot.meta("app.js").accept("./mid.js", (next) => {
      seen.push(next.value);
    });
    require("./mid.js");
  });
  hot.start("app.js", {});
  assert.equal(
    hot.update({
      "mid.js": (module, exports) => {
        midRuns++;
        exports.value = midRuns;
      },
    }),
    true,
  );
  assert.equal(midRuns, 2);
  assert.equal(appRuns, 1);
  assert.deepEqual(seen, [2]);
});

test("invalidate cancels self-accept and bubbles", () => {
  const { hot } = setup();
  let midRuns = 0;
  let appRuns = 0;
  const mid = (invalidate) => (module, exports) => {
    midRuns++;
    const api = hot.meta("mid.js");
    api.accept();
    if (invalidate) api.invalidate();
    exports.value = midRuns;
  };
  hot.define("mid.js", mid(false));
  hot.define("app.js", (module, exports, require) => {
    appRuns++;
    require("./mid.js");
  });
  hot.start("app.js", {});
  assert.equal(hot.update({ "mid.js": mid(true) }), true);
  assert.equal(midRuns, 2);
  assert.equal(appRuns, 2);
});

test("decline fails the update and requests a reload without rerunning", () => {
  const { runtime, hot } = setup();
  let runs = 0;
  hot.define("entry.js", () => {
    runs++;
    hot.meta("entry.js").decline();
  });
  hot.start("entry.js", {});
  assert.equal(
    hot.update({
      "entry.js": () => {
        runs++;
      },
    }),
    false,
  );
  assert.equal(runs, 1);
  assert.equal(runtime.takeReload(), true);
  assert.equal(runtime.takeReload(), false);
});

test("a change to a module that left the graph does nothing and is not a reload", () => {
  const { runtime, hot } = setup();
  hot.define("leaf.js", () => {});
  hot.define("app.js", (module, exports, require) => {
    require("./leaf.js");
  });
  hot.start("app.js", {});
  // The entry stops importing the leaf: pruning removes it from the graph.
  hot.update({ "app.js": () => {} });
  assert.equal(hot.update({ "leaf.js": () => {} }), true);
  assert.equal(runtime.takeReload(), false);
});

test("dispose runs before the next run and data survives", () => {
  const { hot } = setup();
  const log = [];
  let seen;
  const factory = () => {
    const api = hot.meta("entry.js");
    log.push("run");
    seen = api.data.seen;
    api.dispose((data) => {
      data.seen = "yes";
      log.push("dispose");
    });
  };
  hot.define("entry.js", factory);
  hot.start("entry.js", {});
  assert.deepEqual(log, ["run"]);
  assert.equal(seen, undefined);
  assert.equal(hot.update({ "entry.js": factory }), true);
  assert.deepEqual(log, ["run", "dispose", "run"]);
  assert.equal(seen, "yes");
});

test("a throwing accept callback requests a reload", () => {
  const { runtime, hot, errors } = setup();
  hot.define("mid.js", (module, exports) => {
    exports.value = 1;
  });
  hot.define("app.js", (module, exports, require) => {
    hot.meta("app.js").accept("./mid.js", () => {
      throw new Error("accept broke");
    });
    require("./mid.js");
  });
  hot.start("app.js", {});
  assert.equal(
    hot.update({
      "mid.js": (module, exports) => {
        exports.value = 2;
      },
    }),
    false,
  );
  assert.equal(errors[0].message, "accept broke");
  assert.equal(runtime.takeReload(), true);
});

test("prune runs when a module leaves the graph", () => {
  const { hot } = setup();
  const log = [];
  hot.define("leaf.js", () => {
    hot.meta("leaf.js").prune((data) => {
      data.gone = true;
      log.push(["prune", data.gone]);
    });
  });
  hot.define("app.js", (module, exports, require) => {
    require("./leaf.js");
  });
  hot.start("app.js", {});
  assert.equal(hot.update({ "app.js": () => {} }), true);
  assert.deepEqual(log, [["prune", true]]);
});
