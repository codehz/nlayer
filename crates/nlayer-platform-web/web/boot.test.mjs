import assert from "node:assert/strict";
import fs from "node:fs";
import { startPage, stopPage } from "./boot.js";

assert.equal(typeof startPage, "function");
assert.equal(typeof stopPage, "function");

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
assert.match(html, /startPage/);
assert.match(html, /viewport-fit=cover/);
assert.match(html, /nlayer\.wasm/);
