// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// The page's side of the wasm engine. Extracted from nsh's player loop
// and cut to v1: instantiate the module, apply mount JSON with `Player`,
// answer `fetch` with the browser's, and report press, input, scroll,
// resize, safe area and the keyboard inset. A call into the module never
// re-enters it: DOM events that fire while a batch is applied (a scroll
// view set to an offset) wait until the call returns. Text is measured
// by the engine's stand-in, not the browser's fonts.

import { Player } from "./nlayer.js";
import { createFacade, loadBundle } from "./browser/runtime.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Levels `nlayer_log` passes, matching the engine's `log` crate. */
const LEVELS = ["error", "warn", "info", "debug", "debug"];

let wasm = null;
let player = null;
let facade = null;
let releaseBundle = null;
let depth = 0;
const queued = [];
let timer = 0;
let pumpQueued = false;

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

function fillRandom(ptr, len) {
  const crypto = globalThis.crypto;
  if (!wasm || !crypto?.getRandomValues) return 1;
  const bytes = new Uint8Array(memory(), ptr, len);
  let offset = 0;
  while (offset < len) {
    const n = Math.min(65536, len - offset);
    crypto.getRandomValues(bytes.subarray(offset, offset + n));
    offset += n;
  }
  return 0;
}

/** Microseconds since the Unix epoch. QuickJS's wasm shim has no clock of its own. */
function nowUs() {
  return (performance.timeOrigin + performance.now()) * 1000;
}

function importsFor(options) {
  return {
    env: {
      __nlayer_random_fill: fillRandom,
      __rquickjs_host_now_us: nowUs,
    },
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
      nlayer_fetch(id, mp, ml, up, ul, hp, hl, bp, bl, hasBody, follow) {
        const method = text(mp, ml);
        const url = text(up, ul);
        const pairs = JSON.parse(text(hp, hl) || "[]");
        const headers = new Headers();
        for (const [name, value] of pairs) headers.append(name, value);
        const body = hasBody ? new Uint8Array(memory(), bp, bl).slice() : undefined;
        fetch(url, {
          method,
          headers,
          body: body?.length ? body : undefined,
          redirect: follow ? "follow" : "manual",
        }).then(
          async (response) => {
            const bytes = new Uint8Array(await response.arrayBuffer());
            const list = [];
            response.headers.forEach((value, name) => list.push([name, value]));
            const parts = [
              encoder.encode(response.statusText),
              encoder.encode(response.url || url),
              encoder.encode(JSON.stringify(list)),
              bytes,
            ];
            enter(() =>
              withBytes(parts, (sp, sl, rp, rl, jp, jl, dp, dl) => {
                wasm.nlayer_fetch_done(id, 1, response.status, sp, sl, rp, rl, response.redirected ? 1 : 0, jp, jl, dp, dl);
                return undefined;
              }),
            );
          },
          (error) => {
            enter(() =>
              withBytes([encoder.encode(String(error?.message ?? error))], (p, l) => {
                wasm.nlayer_fetch_done(id, 0, 0, p, l, 0, 0, 0, 0, 0, 0, 0);
                return undefined;
              }),
            );
          },
        );
      },
    },
  };
}

async function instantiate(source, options) {
  const response = typeof source === "string" || source instanceof URL ? await fetch(source) : source;
  const bytes = response instanceof Response ? await response.arrayBuffer() : response;
  const module = await WebAssembly.compile(bytes);
  const instance = await WebAssembly.instantiate(module, importsFor(options));
  return instance.exports;
}

function readInsets(root) {
  const probe = root.ownerDocument.createElement("div");
  probe.style.cssText =
    "position:absolute;visibility:hidden;padding:" +
    "env(safe-area-inset-top) env(safe-area-inset-right) " +
    "env(safe-area-inset-bottom) env(safe-area-inset-left)";
  root.append(probe);
  const style = getComputedStyle(probe);
  const n = (value) => parseFloat(value) || 0;
  const insets = [n(style.paddingTop), n(style.paddingRight), n(style.paddingBottom), n(style.paddingLeft)];
  probe.remove();
  return insets;
}

function keyboardInset() {
  const viewport = globalThis.visualViewport;
  if (!viewport) return 0;
  return Math.max(0, globalThis.innerHeight - viewport.height - viewport.offsetTop);
}

function surfaceSize(root) {
  const width = root.clientWidth || globalThis.innerWidth || 0;
  const height = root.clientHeight || globalThis.innerHeight || 0;
  return [width, height];
}

function watchSurface(root) {
  const send = () => {
    const [width, height] = surfaceSize(root);
    const [top, right, bottom, left] = readInsets(root);
    const keyboard = keyboardInset();
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
  if (!wasm) {
    wasm = await instantiate(options.wasm, options);
    watchSurface(root);
  }
  root.replaceChildren();
  player = new Player(root, {
    press(id) {
      enter(() => {
        wasm.nlayer_press(id);
        return wasm.nlayer_pump();
      });
    },
    change(id, value) {
      enter(() =>
        withBytes([encoder.encode(value)], (p, l) => {
          wasm.nlayer_input(id, p, l);
          return wasm.nlayer_pump();
        }),
      );
    },
    scroll(id) {
      enter(() => {
        wasm.nlayer_scroll(id);
        return wasm.nlayer_pump();
      });
    },
  }, { assetBase: options.assetBase ?? "", baseCSS: options.baseCSS });
  const [width, height] = surfaceSize(root);
  const [top, right, bottom, left] = readInsets(root);
  const keyboard = keyboardInset();
  if (typeof wasm.nlayer_browser_initialize === "function") {
    facade = createFacade(wasm, requestPump);
    enter(() => {
      wasm.nlayer_browser_initialize();
      facade.installBoundary();
      wasm.nlayer_browser_start(width, height);
      wasm.nlayer_insets(top, right, bottom, left, keyboard);
      return wasm.nlayer_pump();
    });
    releaseBundle = await loadBundle(options.bundle ?? "", {
      "@nlayer/runtime": facade.runtime,
      "@nlayer/ui": facade.ui,
      "nlayer:rt": facade.rt,
    });
    enter(() => wasm.nlayer_pump());
    return;
  }
  const code = encoder.encode(options.bundle ?? "");
  enter(() =>
    withBytes([code], (p, l) => {
      wasm.nlayer_start(p, l, width, height);
      wasm.nlayer_insets(top, right, bottom, left, keyboard);
      return wasm.nlayer_pump();
    }),
  );
}

/** Stops the running app. The module stays loaded. */
export function stopPage() {
  clearTimeout(timer);
  timer = 0;
  releaseBundle?.();
  releaseBundle = null;
  if (facade) {
    facade.dispose();
    facade = null;
    if (wasm) enter(() => wasm.nlayer_browser_dispose());
  }
  if (wasm) enter(() => wasm.nlayer_stop());
}
