// The module test files import. `nlayer test` inlines it; an app bundle
// refuses the import. `call` reaches the runner through `__nlayerTest`.

const cases = [];

function call(op, payload) {
  const raw = globalThis.__nlayerTest(op, JSON.stringify(payload ?? null));
  return JSON.parse(raw);
}

function show(value) {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function same(a, b) {
  return show(a) === show(b);
}

function fail(message) {
  const error = new Error(message);
  error.name = "Assertion";
  throw error;
}

function thrown(error) {
  const message = error && error.message ? String(error.message) : String(error);
  const stack = error && error.stack ? String(error.stack) : "";
  if (stack === "" || stack.includes(message)) {
    return stack || message;
  }
  return message + "\n" + stack;
}

export function test(name, fn) {
  if (typeof name !== "string" || typeof fn !== "function") {
    throw new TypeError("test(name, fn) needs a name and a function");
  }
  const filter = globalThis.__nlayerFilter;
  if (typeof filter === "string" && filter !== "" && !name.includes(filter)) {
    return;
  }
  cases.push({ name, fn });
}

export function expect(actual) {
  return {
    toBe(expected) {
      if (!Object.is(actual, expected)) {
        fail(`expected ${show(expected)}, got ${show(actual)}`);
      }
    },
    toEqual(expected) {
      if (!same(actual, expected)) {
        fail(`expected ${show(expected)}, got ${show(actual)}`);
      }
    },
    toContain(part) {
      let ok = false;
      if (typeof actual === "string") {
        ok = actual.includes(String(part));
      } else if (Array.isArray(actual)) {
        ok = actual.some((item) => same(item, part));
      }
      if (!ok) {
        fail(`expected ${show(actual)} to contain ${show(part)}`);
      }
    },
    toThrow(pattern) {
      if (typeof actual !== "function") {
        fail(`expected a function, got ${show(actual)}`);
      }
      try {
        actual();
      } catch (error) {
        const message = error && error.message ? String(error.message) : String(error);
        if (pattern !== undefined && !message.includes(String(pattern))) {
          fail(`expected a throw containing ${show(pattern)}, got ${show(message)}`);
        }
        return;
      }
      fail("expected a throw");
    },
  };
}

export function open(entry, options) {
  const id = call("open", { entry, options: options ?? null });
  return {
    texts() {
      return call("texts", id);
    },
    inputs() {
      return call("inputs", id);
    },
    press(selector) {
      call("press", { id, selector });
    },
    type(selector, text) {
      call("type", { id, selector, text });
    },
    select(selector, start, end = start) {
      call("select", { id, selector, start, end });
    },
    compose(selector, text, composing) {
      call("compose", { id, selector, text, composing });
    },
    scroll(selector, distance) {
      call("scroll", { id, selector, distance });
    },
    waitFor(selector) {
      call("waitFor", { id, selector });
    },
    tree() {
      return call("tree", id);
    },
    close() {
      call("close", id);
    },
  };
}

// Runs the tests `test` registered. `suite` is the test file's namespace:
// the entry imports it so the file is evaluated.
export function finish(suite) {
  void suite;
  const results = [];
  for (const item of cases) {
    try {
      const value = item.fn();
      if (value && typeof value.then === "function") {
        throw new Error("tests are synchronous");
      }
      results.push({ name: item.name, ok: true });
    } catch (error) {
      results.push({ name: item.name, ok: false, error: thrown(error) });
    } finally {
      call("end", null);
    }
  }
  globalThis.__nlayerResult = JSON.stringify(results);
}
