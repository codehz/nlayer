import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { createFacade } from "./runtime.js";

const wasmPath = new URL(
  "../../../../target/wasm32-unknown-unknown/debug/nlayer_platform_web.wasm",
  import.meta.url,
);

test("the page graph and a text mount do not need QuickJS", async () => {
  const bytes = await readFile(wasmPath);
  const module = await WebAssembly.compile(bytes);
  const imported = WebAssembly.Module.imports(module).map((entry) => `${entry.module}.${entry.name}`);
  assert.equal(imported.some((name) => /quickjs|rquickjs/i.test(name)), false);
  const exported = WebAssembly.Module.exports(module).map((entry) => entry.name);
  assert.equal(exported.includes("nlayer_browser_initialize"), true);
  assert.equal(exported.includes("nlayer_start"), false);
  const binary = Buffer.from(bytes).toString("latin1");
  assert.equal(binary.includes("__rquickjs"), false);
  assert.equal(binary.includes("QuickJS"), false);
  const frames = [];
  let facade;
  const imports = {
    nlayer_browser: {
      value: (op, a, b, c, number) => facade.imports.value(op, a, b, c, number),
    },
    nlayer: {
      nlayer_submit: (ptr, len) => {
        const text = new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, ptr, len));
        frames.push(JSON.parse(text));
      },
      nlayer_request_pump: () => {},
      nlayer_report_error: (ptr, len) => {
        const text = new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, ptr, len));
        throw new Error(text);
      },
      nlayer_log: () => {},
    },
  };
  const instance = await WebAssembly.instantiate(module, imports);
  const wasm = instance.exports;
  assert.equal(typeof wasm.nlayer_browser_initialize, "function");
  facade = createFacade(wasm, () => {});
  wasm.nlayer_browser_initialize();
  facade.installBoundary();

  const count = facade.runtime.signal(1);
  const doubled = facade.runtime.computed(() => count() * 2);
  const log = [];
  facade.runtime.effect(() => log.push(`${count()}/${doubled()}`));
  count.set(2);
  count.update((n) => n + 1);
  assert.equal(log.join(","), "1/2,2/4,3/6");

  wasm.nlayer_browser_start(320, 480);
  facade.runtime.mount(() => {
    const text = facade.rt.element("text");
    facade.rt.append(text, facade.rt.text("hello"));
    return text;
  });
  wasm.nlayer_pump();
  assert.ok(frames.length > 0, "a frame was submitted");
  assert.match(JSON.stringify(frames.at(-1)), /hello/);
});
