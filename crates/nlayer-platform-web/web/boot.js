// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// The page's side of the wasm engine. The module does not link QuickJS:
// this file evaluates the app (`browser/runtime.js`) and the module lays
// out frames. Instantiate the module, apply mount JSON with `Player`,
// and report press, input, scroll, resize, safe area and the keyboard
// inset. The app's `fetch` is the page's. A call into the module never
// re-enters it: DOM events that fire while a batch is applied (a scroll
// view set to an offset) wait until the call returns. Text is measured
// by the engine's stand-in, not the browser's fonts.

import { Player } from "./nlayer.js";
import { installHost } from "./host.js";
import { createFacade, loadBundle, loadDevScript } from "./browser/runtime.js";

// A failed start would otherwise only time out in `nlayer test`.
globalThis.addEventListener?.("error", (event) => {
  if (globalThis.__nlayerReady !== true) {
    globalThis.__nlayerFailed = String(event.error?.stack ?? event.message ?? event);
  }
});
globalThis.addEventListener?.("unhandledrejection", (event) => {
  if (globalThis.__nlayerReady !== true) {
    const reason = event.reason;
    globalThis.__nlayerFailed = String(reason?.stack ?? reason?.message ?? reason);
  }
});

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Levels `nlayer_log` passes, matching the engine's `log` crate. */
const LEVELS = ["error", "warn", "info", "debug", "debug"];

/** `EventKind` discriminants. `onScroll` is 1. */
const SCROLL_KIND = {
  onScroll: 1,
  onScrollEnd: 2,
  onScrollDragStart: 3,
  onScrollDragEnd: 4,
  onMomentumStart: 5,
  onMomentumEnd: 6,
};

let wasm = null;
let player = null;
let facade = null;
let releaseBundle = null;
let depth = 0;
const queued = [];
let timer = 0;
let pumpQueued = false;
// The development page's socket and the session that owns it.
let devSocket = null;
let devGeneration = 0;

function memory() {
  return wasm.memory.buffer;
}

function text(ptr, len) {
  if (!len) return "";
  return decoder.decode(new Uint8Array(memory(), ptr, len));
}

/** Writes `parts` (`Uint8Array`) and calls `f(ptr, len, ...)`, then frees them. */
function withBytes(parts, f) {
  const stored = parts.map((data) => {
    const ptr = wasm.nlayer_alloc(data.length);
    if (data.length) new Uint8Array(memory(), ptr, data.length).set(data);
    return [ptr, data.length];
  });
  try {
    return f(...stored.flat());
  } finally {
    for (const [ptr, len] of stored) wasm.nlayer_dealloc(ptr, len);
  }
}

function arm(delay) {
  if (typeof delay !== "number") return;
  clearTimeout(timer);
  timer = 0;
  if (delay >= 0) timer = setTimeout(() => enter(() => wasm.nlayer_pump()), delay);
}

/** Runs `fn` against the module, or queues it when a call is already in progress. */
function enter(fn) {
  if (depth) {
    queued.push(fn);
    return undefined;
  }
  depth++;
  let delay;
  try {
    delay = fn();
  } finally {
    depth--;
  }
  arm(delay);
  while (!depth && queued.length) {
    const next = queued.shift();
    depth++;
    try {
      delay = next();
    } finally {
      depth--;
    }
    arm(delay);
  }
  return delay;
}

function requestPump() {
  if (pumpQueued) return;
  pumpQueued = true;
  queueMicrotask(() => {
    pumpQueued = false;
    if (wasm) enter(() => wasm.nlayer_pump());
  });
}

let frameQueued = false;
/** The engine waits for a display frame: pump on the page's next animation frame. */
function requestFrame() {
  if (frameQueued) return;
  frameQueued = true;
  const run = () => {
    frameQueued = false;
    enter(() => wasm.nlayer_pump());
  };
  if (typeof globalThis.requestAnimationFrame === "function") {
    globalThis.requestAnimationFrame(run);
  } else {
    setTimeout(run, 16);
  }
}

// Blink's shape cache stays near 5 MB if a context is dropped this often.
const MEASURE_ROUNDS = 5000;
let measureUses = 0;
let measureCtx = null;
const metricsCache = new Map();
let numericSpan = null;

function measureContext() {
  const canvas =
    typeof OffscreenCanvas !== "undefined"
      ? new OffscreenCanvas(1, 1)
      : globalThis.document.createElement("canvas");
  return canvas.getContext("2d");
}

function measuring() {
  if (!measureCtx || ++measureUses >= MEASURE_ROUNDS) {
    measureUses = 0;
    measureCtx = measureContext();
  }
  return measureCtx;
}

function fontMetrics(font) {
  let cached = metricsCache.get(font);
  if (!cached) {
    const ctx = measuring();
    ctx.font = font;
    const sample = ctx.measureText("Hg");
    const ex = ctx.measureText("x");
    cached = [
      sample.fontBoundingBoxAscent ?? 0,
      sample.fontBoundingBoxDescent ?? 0,
      ex.actualBoundingBoxAscent ?? 0,
    ];
    metricsCache.set(font, cached);
  }
  return cached;
}

/** Canvas cannot set `font-variant-numeric`; a hidden span can. */
function measureNumeric(font, numeric, value) {
  const doc = globalThis.document;
  if (!doc?.body) return null;
  if (!numericSpan?.isConnected) {
    const box = doc.createElement("div");
    box.setAttribute("aria-hidden", "true");
    box.style.cssText =
      "position:fixed;left:0;top:0;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none;contain:strict";
    numericSpan = doc.createElement("span");
    numericSpan.style.cssText =
      "position:absolute;left:0;top:0;white-space:pre;letter-spacing:normal;word-spacing:normal;text-transform:none;font-kerning:auto;font-feature-settings:normal";
    box.append(numericSpan);
    doc.body.append(box);
  }
  numericSpan.style.letterSpacing = "normal";
  numericSpan.style.font = font;
  numericSpan.style.fontVariantNumeric = numeric;
  numericSpan.textContent = value;
  return numericSpan.getBoundingClientRect().width;
}

function clusterBoxes(font, numeric, spacing, value, ranges) {
  const doc = globalThis.document;
  if (!doc?.body || measureNumeric(font, numeric || "normal", value) == null) {
    return ranges.map(() => [0, 0]);
  }
  numericSpan.style.direction = "ltr";
  numericSpan.style.letterSpacing = `${spacing}px`;
  const origin = numericSpan.getBoundingClientRect().left;
  const node = numericSpan.firstChild;
  if (!node) return ranges.map(() => [0, 0]);
  const range = doc.createRange();
  return ranges.map(([start, end]) => {
    range.setStart(node, start);
    range.setEnd(node, end);
    const box = range.getBoundingClientRect();
    return [box.left - origin, box.right - origin];
  });
}

function importsFor(options) {
  return {
    nlayer_browser: {
      value(op, a, b, c, number) {
        if (!facade) throw new Error("the browser host was not initialized");
        return facade.imports.value(op, a, b, c, number);
      },
    },
    nlayer: {
      nlayer_submit(ptr, len) {
        player?.apply(JSON.parse(text(ptr, len)));
      },
      nlayer_request_pump: requestPump,
      nlayer_request_frame: requestFrame,
      nlayer_report_error(ptr, len) {
        const message = text(ptr, len);
        options.onError?.(message);
        console.error(message);
      },
      nlayer_log(level, ptr, len) {
        const message = text(ptr, len);
        const name = LEVELS[level] ?? "log";
        (console[name] ?? console.log).call(console, message);
        options.onLog?.(name, message);
      },
      measure_text(fp, fl, tp, tl) {
        const ctx = measuring();
        ctx.font = text(fp, fl);
        return ctx.measureText(text(tp, tl)).width;
      },
      measure_numeric_text(fp, fl, np, nl, tp, tl) {
        const font = text(fp, fl);
        const value = text(tp, tl);
        const width = measureNumeric(font, text(np, nl), value);
        if (width != null) return width;
        const ctx = measuring();
        ctx.font = font;
        return ctx.measureText(value).width;
      },
      text_cluster_boxes(fp, fl, np, nl, spacing, tp, tl, rp, op, count) {
        const input = new Uint32Array(memory(), rp, count * 2);
        const ranges = Array.from({ length: count }, (_, i) => [input[2 * i], input[2 * i + 1]]);
        const boxes = clusterBoxes(text(fp, fl), text(np, nl), spacing, text(tp, tl), ranges);
        const output = new Float32Array(memory(), op, count * 2);
        boxes.forEach(([left, right], i) => {
          output[2 * i] = left;
          output[2 * i + 1] = right;
        });
      },
      font_ascent(fp, fl) {
        return fontMetrics(text(fp, fl))[0];
      },
      font_descent(fp, fl) {
        return fontMetrics(text(fp, fl))[1];
      },
      font_x_height(fp, fl) {
        return fontMetrics(text(fp, fl))[2];
      },
    },
  };
}

async function instantiate(source, options) {
  const response =
    typeof source === "string" || source instanceof URL ? await fetch(source) : source;
  const bytes = response instanceof Response ? await response.arrayBuffer() : response;
  const module = await WebAssembly.compile(bytes);
  const provided = importsFor(options);
  for (const entry of WebAssembly.Module.imports(module)) {
    if (/quickjs|rquickjs/i.test(`${entry.module}.${entry.name}`)) {
      throw new Error("nlayer.wasm links QuickJS; the page evaluates the app");
    }
    const namespace = provided[entry.module];
    if (!namespace || typeof namespace[entry.name] !== "function") {
      throw new Error(
        `nlayer.wasm imports ${entry.module}.${entry.name}, which the page does not provide`,
      );
    }
  }
  const instance = await WebAssembly.instantiate(module, provided);
  return instance.exports;
}

let pinned = null;

function readInsets(root) {
  if (pinned) {
    const area = pinned.safeArea ?? {};
    return [area.top || 0, area.right || 0, area.bottom || 0, area.left || 0];
  }
  const probe = root.ownerDocument.createElement("div");
  probe.style.cssText =
    "position:absolute;visibility:hidden;padding:" +
    "env(safe-area-inset-top) env(safe-area-inset-right) " +
    "env(safe-area-inset-bottom) env(safe-area-inset-left)";
  root.append(probe);
  const style = getComputedStyle(probe);
  const n = (value) => parseFloat(value) || 0;
  const insets = [
    n(style.paddingTop),
    n(style.paddingRight),
    n(style.paddingBottom),
    n(style.paddingLeft),
  ];
  probe.remove();
  return insets;
}

function keyboardInset() {
  if (pinned) return pinned.keyboard || 0;
  const viewport = globalThis.visualViewport;
  if (!viewport) return 0;
  return Math.max(0, globalThis.innerHeight - viewport.height - viewport.offsetTop);
}

function surfaceSize(root) {
  if (pinned?.width > 0 && pinned?.height > 0) return [pinned.width, pinned.height];
  const width = root.clientWidth || globalThis.innerWidth || 0;
  const height = root.clientHeight || globalThis.innerHeight || 0;
  return [width, height];
}

function watchSurface(root) {
  const send = () => {
    const [width, height] = surfaceSize(root);
    const [top, right, bottom, left] = readInsets(root);
    const keyboard = keyboardInset();
    player?.setKeyboard?.(keyboard);
    enter(() => {
      wasm.nlayer_resize(width, height);
      wasm.nlayer_insets(top, right, bottom, left, keyboard);
      return wasm.nlayer_pump();
    });
  };
  if (typeof ResizeObserver === "function") new ResizeObserver(send).observe(root);
  else globalThis.addEventListener?.("resize", send);
  globalThis.visualViewport?.addEventListener("resize", send);
  globalThis.visualViewport?.addEventListener("scroll", send);
}

/**
 * Loads the wasm engine (once) and runs `options.bundle` in `root`.
 * `options.wasm` is a URL, a `Response` or the module's bytes.
 * `options.baseCSS` is the mounter's base sheet. `options.assetBase`
 * is prepended to `asset:/` image URIs. A running app is replaced;
 * its state is dropped. Returns when the first frame has been applied.
 */
export async function startPage(root, options = {}) {
  if (options.pin) pinned = options.pin;
  if (!wasm) {
    wasm = await instantiate(options.wasm, options);
    watchSurface(root);
  } else {
    // A running app is replaced: its graph, realm and views are dropped
    // first. The document itself stays.
    teardown();
  }
  root.replaceChildren();
  player = new Player(
    root,
    {
      press(id) {
        enter(() => {
          wasm.nlayer_press(id);
          return wasm.nlayer_pump();
        });
      },
      states(id, bits) {
        enter(() => {
          wasm.nlayer_states(id, bits);
          return wasm.nlayer_pump();
        });
      },
      longPress(id) {
        enter(() => {
          wasm.nlayer_long_press(id);
          return wasm.nlayer_pump();
        });
      },
      pan(id, phase, tx, ty, vx, vy, value_x, value_y) {
        if (!wasm) return;
        enter(() => {
          wasm.nlayer_pan(id, phase, tx, ty, vx, vy, value_x, value_y);
          return wasm.nlayer_pump();
        });
      },
      transformGesture(id, rotation, phase, amount, velocity, value) {
        if (!wasm) return;
        enter(() => {
          wasm.nlayer_transform_gesture(id, rotation, phase, amount, velocity, value);
          return wasm.nlayer_pump();
        });
      },
      snap(
        offset,
        velocity,
        viewport,
        content,
        paging,
        snapInterval,
        snapOffsets,
        snapAlignment,
        disableIntervalMomentum,
        deceleration,
      ) {
        if (!wasm) return -1.0;
        if (!snapOffsets || snapOffsets.length === 0) {
          return wasm.nlayer_snap(
            offset,
            velocity,
            viewport,
            content,
            paging,
            snapInterval,
            0,
            0,
            snapAlignment,
            disableIntervalMomentum,
            deceleration,
          );
        }
        // Copy offsets to wasm memory temporarily
        const bytes = snapOffsets.length * 4;
        const ptr = wasm.nlayer_alloc(bytes) ?? 0;
        if (ptr === 0) {
          return wasm.nlayer_snap(
            offset,
            velocity,
            viewport,
            content,
            paging,
            snapInterval,
            0,
            0,
            snapAlignment,
            disableIntervalMomentum,
            deceleration,
          );
        }
        const view = new Float32Array(wasm.memory.buffer, ptr, snapOffsets.length);
        view.set(snapOffsets);
        const result = wasm.nlayer_snap(
          offset,
          velocity,
          viewport,
          content,
          paging,
          snapInterval,
          ptr,
          snapOffsets.length,
          snapAlignment,
          disableIntervalMomentum,
          deceleration,
        );
        wasm.nlayer_dealloc(ptr, bytes);
        return result;
      },
      change(id, value) {
        enter(() =>
          withBytes([encoder.encode(value)], (p, l) => {
            wasm.nlayer_input(id, p, l);
            return wasm.nlayer_pump();
          }),
        );
      },
      selection(id, start, end) {
        enter(() => {
          wasm.nlayer_selection(id, start, end);
          return wasm.nlayer_pump();
        });
      },
      composition(id, composing, text) {
        enter(() =>
          withBytes([encoder.encode(text)], (p, l) => {
            wasm.nlayer_composition(id, composing ? 1 : 0, p, l);
            return wasm.nlayer_pump();
          }),
        );
      },
      scroll(id, kind, x, y, contentW, contentH, width, height) {
        const raw = SCROLL_KIND[kind];
        if (raw == null) return;
        enter(() => {
          wasm.nlayer_scroll(id, raw, x, y, contentW, contentH, width, height);
          return wasm.nlayer_pump();
        });
      },
      back() {
        enter(() => {
          wasm.nlayer_back();
          return wasm.nlayer_pump();
        });
      },
      setRootSize(id, width, height, top, right, bottom, left) {
        enter(() => {
          wasm.nlayer_root_size(id, width, height, top, right, bottom, left);
          return wasm.nlayer_pump();
        });
      },
    },
    { assetBase: options.assetBase ?? "", assetMap: options.assetMap, baseCSS: options.baseCSS },
  );
  const [width, height] = surfaceSize(root);
  const [top, right, bottom, left] = readInsets(root);
  const keyboard = keyboardInset();
  player.setKeyboard(keyboard);
  if (
    typeof wasm.nlayer_browser_initialize !== "function" ||
    typeof wasm.nlayer_start === "function"
  ) {
    throw new Error("nlayer.wasm must be the browser host and must not export nlayer_start");
  }
  facade = createFacade(wasm, requestPump, {
    appId: options.appId,
    colorScheme: options.colorScheme,
    dev: options.dev === true,
    report: options.report,
  });
  player.setAnimation(facade.animation);
  enter(() => {
    wasm.nlayer_browser_initialize();
    facade.installBoundary();
    wasm.nlayer_browser_start(width, height);
    wasm.nlayer_insets(top, right, bottom, left, keyboard);
    return wasm.nlayer_pump();
  });
  if (options.devSource !== undefined && options.devSource !== "") {
    // A development bundle: module factories started through `__nlayer_hot`.
    releaseBundle = await loadDevScript(options.devSource, options.devName);
  } else if (options.devSource === undefined) {
    releaseBundle = await loadBundle(options.bundle ?? "", {
      "@nlayer/runtime": facade.runtime,
      "@nlayer/ui": facade.ui,
      "nlayer:rt": facade.rt,
    });
  }
  enter(() => wasm.nlayer_pump());
  installHost(player, root);
  // A dev page's idle start (no bundle yet) is not the app.
  if (!options.idle) globalThis.__nlayerReady = true;
  globalThis.__nlayerFailed = "";
  return facade;
}

/** Drops the running app's realm, graph and views. The module stays
 * loaded, and so does the document. */
function teardown() {
  releaseBundle?.();
  releaseBundle = null;
  // The wasm side drops values whose finalizer calls `facade.imports`, so
  // the facade must outlive both wasm calls.
  if (wasm) {
    enter(() => wasm.nlayer_browser_dispose());
    enter(() => wasm.nlayer_stop());
  }
  if (facade) {
    facade.dispose();
    facade = null;
  }
  player?.dispose();
  player = null;
}

/** Stops the running app. The module stays loaded. */
export function stopPage() {
  clearTimeout(timer);
  timer = 0;
  devGeneration++;
  devSocket?.close();
  devSocket = null;
  removeProblem();
  teardown();
}

/** The protocol version this page speaks (`nlayer-engine` protocol). */
const PROTOCOL = 1;

function devUrl(given) {
  if (given) return given;
  const scheme = globalThis.location?.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${globalThis.location.host}/ws`;
}

function showStatus(message) {
  const status = globalThis.document?.getElementById("status");
  if (!status) return;
  status.textContent = message;
  status.style.display = message ? "" : "none";
}

// The build diagnostic covers the app; it is not a UI component.
function showProblem(message) {
  const doc = globalThis.document;
  if (!doc) return;
  let element = doc.getElementById("nlayer-problem");
  if (!element) {
    element = doc.createElement("div");
    element.id = "nlayer-problem";
    element.setAttribute("role", "alert");
    element.style.cssText =
      "position:fixed;inset:0;z-index:2147483647;overflow:auto;padding:24px;" +
      "background:rgba(20,20,22,.94);color:#f2f2f7;" +
      'font:14px/1.4 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;';
    const heading = doc.createElement("h1");
    heading.textContent = "Build failed";
    heading.style.cssText = "margin:0 0 12px;font-size:17px;color:#ff6b5f;";
    const detail = doc.createElement("pre");
    detail.style.cssText =
      "margin:0;white-space:pre-wrap;user-select:text;-webkit-user-select:text;" +
      "font:12px/1.5 ui-monospace,Menlo,Consolas,monospace;";
    element.append(heading, detail);
    doc.body.append(element);
  }
  element.querySelector("pre").textContent = message;
}

function removeProblem() {
  globalThis.document?.getElementById("nlayer-problem")?.remove();
}

/** A hot update script's completion value, as the engine's `eval_script`:
 * a classic script in the page's realm, not the caller's scope. */
function evalPatch(source, name) {
  const label = String(name).replace(/[\r\n]/g, "");
  return (0, eval)(`${source}\n//# sourceURL=${label}\n`);
}

/**
 * The development page (`nlayer run --target web`): starts the engine,
 * connects to the dev server's `/ws`, runs its factory bundles and
 * applies patches in place. A build failure covers the app; the next
 * success uncovers it. Nothing here reloads the document.
 *
 * `options.devUrl` defaults to the same origin's `/ws`. `options.onError`
 * shows errors the engine cannot draw.
 */
export async function startDevPage(root, options = {}) {
  const report = (error) => console.error("[nlayer]", error);
  const page = { ...options, dev: true, report };
  const generation = ++devGeneration;
  devSocket?.close();
  devSocket = null;
  // The build the page runs; null until a bundle started.
  let running = null;
  // The bundle being started, if any.
  let loading = null;
  // A test session (`hello.test`): no overlay, no patches.
  let test = false;
  let retry = 100;
  // The message whose binary frames are still coming.
  let pending = null;
  let chain = Promise.resolve();

  const enqueue = (task) => {
    chain = chain.then(task).catch((error) => {
      page.onError?.(String(error?.stack ?? error?.message ?? error));
    });
    return chain;
  };

  // One bundle, in place: the engine and the realm are rebuilt, the
  // document is not.
  const mount = (source, name) => startPage(root, { ...page, devSource: source, devName: name });

  const onFrame = async (message, frames) => {
    const script = frames[0] ? new TextDecoder().decode(frames[0]) : "";
    if (message.type === "patch") {
      if (test || loading !== null || running !== message.base) return;
      let applied = false;
      try {
        applied = evalPatch(script, `hot-${message.version}.js`) === true;
      } catch (error) {
        globalThis.__nlayerFailed = String(error?.stack ?? error);
        report(error);
      }
      const hot = facade?.hot ?? null;
      const reload = hot ? hot.takeReload() : false;
      if (applied && !reload) {
        running = message.version;
        removeProblem();
        showStatus("");
        if (player) enter(() => wasm.nlayer_pump());
        return;
      }
      // The bundle of this version follows and replaces the app; until it
      // arrives the last good app stays.
      running = null;
      return;
    }
    if (message.type !== "bundle") return;
    if (running === message.version) {
      // A reconnect's snapshot of the build already running.
      showStatus("");
      return;
    }
    if (test && running !== null) return;
    loading = message.version;
    try {
      await mount(script, `bundle-${message.version}.js`);
      running = message.version;
      removeProblem();
      showStatus("");
    } catch (error) {
      running = null;
      const text = String(error?.stack ?? error?.message ?? error);
      globalThis.__nlayerFailed = text;
      page.onError?.(text);
    } finally {
      loading = null;
    }
  };

  const onControl = (message) => {
    if (message.type === "hello") {
      test = message.test === true;
      return;
    }
    if (message.type === "build_failed" && !test) {
      showStatus("");
      showProblem(String(message.message ?? ""));
    }
  };

  const connect = () => {
    if (generation !== devGeneration) return;
    const socket = new WebSocket(devUrl(options.devUrl));
    devSocket = socket;
    socket.binaryType = "arraybuffer";
    socket.onopen = () => {
      if (generation !== devGeneration) return;
      retry = 100;
      pending = null;
      socket.send(JSON.stringify({ type: "hello", protocol: PROTOCOL, test: false }));
    };
    socket.onmessage = (event) => {
      if (typeof event.data !== "string") {
        if (!pending) return;
        pending.frames.push(new Uint8Array(event.data));
        if (pending.frames.length < pending.count) return;
        const { message, frames } = pending;
        pending = null;
        enqueue(() => onFrame(message, frames));
        return;
      }
      let message;
      try {
        message = JSON.parse(event.data);
      } catch (error) {
        report(error);
        return;
      }
      if (message.type === "patch" || message.type === "bundle") {
        pending = { message, frames: [], count: message.source_map ? 2 : 1 };
        return;
      }
      enqueue(() => onControl(message));
    };
    socket.onclose = () => {
      if (generation !== devGeneration) return;
      devSocket = null;
      showStatus("Dev server disconnected — reconnecting…");
      setTimeout(connect, retry);
      retry = Math.min(retry * 2, 2000);
    };
  };

  // The engine starts as the packaged page's does; the first bundle
  // replaces this empty app in place and is the first `ready` page.
  await startPage(root, { ...page, devSource: "", devName: "bundle.js", idle: true });
  showStatus("Connecting…");
  connect();
}
