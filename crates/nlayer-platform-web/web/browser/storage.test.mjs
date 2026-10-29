// The page's `storage`: `localStorage` entries under one prefix, the
// core's quota, and the same promises as the QuickJS host.

import assert from "node:assert/strict";
import test from "node:test";

import { QUOTA, createStorage, storagePrefix } from "./storage.js";

class Area {
  constructor() {
    this.entries = new Map();
  }
  get length() {
    return this.entries.size;
  }
  key(i) {
    return [...this.entries.keys()][i] ?? null;
  }
  getItem(key) {
    return this.entries.has(key) ? this.entries.get(key) : null;
  }
  setItem(key, value) {
    this.entries.set(key, String(value));
  }
  removeItem(key) {
    this.entries.delete(key);
  }
}

test("the prefix is the app id, else the page directory", () => {
  assert.equal(storagePrefix("com.example.todo", "https://x.org/"), "nlayer:com.example.todo:");
  assert.equal(
    storagePrefix(undefined, "https://x.com/apps/todo/index.html?a#b"),
    "nlayer:/apps/todo/:",
  );
  assert.equal(storagePrefix("", "https://x.com/apps/todo/"), "nlayer:/apps/todo/:");
  assert.equal(storagePrefix("a:b", "https://x.com/apps/todo/"), "nlayer:/apps/todo/:");
  assert.equal(storagePrefix(undefined, "https://x.com"), "nlayer:/:");
  assert.equal(storagePrefix(undefined, "https://x.com?q=/a/b"), "nlayer:/:");
  assert.notEqual(
    storagePrefix(undefined, "https://x.com/a/"),
    storagePrefix(undefined, "https://x.com/b/"),
  );
});

test("reads and writes are promises over prefixed entries", async () => {
  const area = new Area();
  const storage = createStorage(area, "nlayer:app:");
  assert.equal(await storage.getItem("k"), null);
  await storage.setItem("b", "2");
  await storage.setItem("a", "1");
  assert.equal(area.getItem("nlayer:app:a"), "1");
  assert.deepEqual(await storage.keys(), ["a", "b"]);
  assert.deepEqual(area.entries.size, 2);
  await storage.removeItem("a");
  assert.deepEqual(await storage.keys(), ["b"]);
  await storage.clear();
  assert.deepEqual(await storage.keys(), []);
  assert.equal(area.entries.size, 0);
});

test("another app's entries are not seen or cleared", async () => {
  const area = new Area();
  const first = createStorage(area, "nlayer:one:");
  const second = createStorage(area, "nlayer:two:");
  await first.setItem("k", "one");
  await second.setItem("k", "two");
  assert.equal(await first.getItem("k"), "one");
  await first.clear();
  assert.equal(await second.getItem("k"), "two");
});

test("a write past the quota rejects and keeps the old value", async () => {
  const area = new Area();
  const storage = createStorage(area, "nlayer:app:");
  await storage.setItem("k", "x".repeat(QUOTA - 1));
  assert.equal(await storage.getItem("k"), "x".repeat(QUOTA - 1));
  await assert.rejects(storage.setItem("j", "y"), (error) => {
    assert.equal(error.name, "QuotaExceededError");
    assert.equal(error.code, "ERR_NLAYER_QUOTA_EXCEEDED");
    return true;
  });
  assert.equal(await storage.getItem("j"), null);
  // The quota counts the new value only when a key is replaced.
  await storage.setItem("k", "z".repeat(QUOTA - 1));
  assert.equal((await storage.getItem("k")).length, QUOTA - 1);
  // UTF-8 bytes, not UTF-16 units: "键" is three bytes.
  const area2 = new Area();
  const other = createStorage(area2, "nlayer:app:");
  await other.setItem("键", "😀".repeat((QUOTA - 3) / 4));
  await assert.rejects(other.setItem("x", "y"));
});
