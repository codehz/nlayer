// The page's animation graph. The app's modules run here, so the graph and
// its display clock run here too; JS only declares the values. Bindings
// arrive from the wasm core (the style decoder turned an animated value
// into a binding) and this file writes the evaluated properties to the DOM.
//
// The graph matches `nlayer-core`'s: values, piecewise interpolation, and
// the timing and spring drivers. Colors are `0xRRGGBBAA`.

const FRAME = 1000 / 60;

export function createAnimation() {
  const nodes = new Map();
  const views = new Map();
  const finalizer = new FinalizationRegistry((id) => release(id));
  let nextNode = 2;
  let nextAnimation = 2;
  let frame = 0;

  function allocateNode() {
    const id = nextNode;
    nextNode += 2;
    if (id >= 0xfffffffe) throw new RangeError("animation IDs exhausted");
    return id;
  }

  function allocateAnimation() {
    const id = nextAnimation;
    nextAnimation += 2;
    if (id >= 0xfffffffe) throw new RangeError("animation IDs exhausted");
    return id;
  }

  function makeNode(proto, id, inputs) {
    const node = Object.create(proto);
    Object.defineProperty(node, "__av", { value: id });
    if (inputs.length) Object.defineProperty(node, "__inputs", { value: inputs });
    finalizer.register(node, id);
    return node;
  }

  function finish(id, finished) {
    const node = nodes.get(id === undefined ? -1 : id);
    void node;
  }

  function resolveAnimation(animation, finished, value) {
    const end = animations.get(animation);
    if (!end) return;
    animations.delete(animation);
    end.resolve(end.read ? value : { finished });
  }

  const animations = new Map();

  function createValue(value) {
    const id = allocateNode();
    nodes.set(id, { kind: "value", value: Number(value) || 0, velocity: 0, running: null });
    return id;
  }

  function createInterpolate(input, config) {
    const id = allocateNode();
    if (!Array.isArray(config.inputRange) || !Array.isArray(config.outputRange)) {
      throw new TypeError("interpolate: inputRange and outputRange must be arrays");
    }
    const colors = config.outputRange.length > 0 && typeof config.outputRange[0] !== "number";
    const output = colors ? config.outputRange.map(parseColor) : config.outputRange.map(Number);
    nodes.set(id, {
      kind: "interpolate",
      input,
      inputRange: config.inputRange.map(Number),
      output,
      colors,
      left: config.extrapolateLeft === "extend" || config.extrapolate === "extend" ? 1 : 0,
      right: config.extrapolateRight === "extend" || config.extrapolate === "extend" ? 1 : 0,
      easing: parseEasing(config.easing),
    });
    return id;
  }

  function valueOf(id, depth = 0) {
    if (depth > 64) return 0;
    const node = nodes.get(id);
    if (!node) return 0;
    if (node.kind === "value") return node.value;
    return interpolate(valueOf(node.input, depth + 1), node);
  }

  function interpolate(x, node) {
    const range = node.inputRange;
    const output = node.output;
    const n = Math.min(range.length, output.length);
    if (n === 0) return x;
    const first = range[0];
    const last = range[n - 1];
    const side = x < first ? node.left : x > last ? node.right : -1;
    if (side === 0) return output[x < first ? 0 : n - 1];
    if (n === 1) return output[0];
    let segment = -1;
    if (range.every((v, i) => i === 0 || v >= range[i - 1])) {
      let lo = 0;
      let hi = n - 1;
      while (lo + 1 < hi) {
        const mid = (lo + hi) >> 1;
        if (range[mid] < x) lo = mid;
        else hi = mid;
      }
      segment = Math.min(lo, n - 2);
    } else {
      for (let i = 1; i < n; i++) {
        if (x <= range[i]) {
          segment = i - 1;
          break;
        }
      }
      if (segment < 0) segment = n - 2;
    }
    const a = range[segment];
    const b = range[segment + 1];
    let t = b === a ? 0 : (x - a) / (b - a);
    if (node.easing.kind !== "linear" && t >= 0 && t <= 1) t = ease(node.easing, t);
    if (node.colors) return lerpColor(output[segment], output[segment + 1], t);
    const from = output[segment];
    return from + (output[segment + 1] - from) * t;
  }

  function start(id, driver) {
    const node = nodes.get(id);
    if (!node || node.kind !== "value") throw new TypeError("not an animated value");
    if (node.running) resolveAnimation(node.running.animation, false);
    node.running = {
      animation: allocateAnimation(),
      driver,
      from: node.value,
      velocity:
        driver.kind === "spring" && driver.velocity !== null ? driver.velocity : node.velocity,
      start: null,
    };
    const promise = new Promise((resolve) => {
      animations.set(node.running.animation, { resolve, read: false });
    });
    wake();
    return promise;
  }

  function stopNode(id, finish) {
    const node = nodes.get(id);
    const running = node?.running;
    if (running) {
      resolveAnimation(running.animation, false);
      node.running = null;
    }
    if (node) node.velocity = 0;
    void finish;
  }

  function release(id) {
    stopNode(id);
    nodes.delete(id);
  }

  function tick(now) {
    for (const node of nodes.values()) {
      if (node.kind !== "value" || !node.running) continue;
      const run = node.running;
      if (run.start === null) run.start = now;
      const elapsed = Math.max(0, now - run.start);
      const driver = run.driver;
      let value;
      let velocity = 0;
      let done = false;
      if (driver.kind === "timing") {
        const t = elapsed * 1000 - driver.delay;
        if (t < 0) {
          value = run.from;
        } else if (t >= driver.duration) {
          value = driver.to;
          done = true;
        } else {
          value = run.from + (driver.to - run.from) * ease(driver.easing, t / driver.duration);
        }
      } else {
        const result = spring(
          run.from - driver.to,
          run.velocity,
          driver.stiffness,
          driver.damping,
          driver.mass,
          elapsed,
        );
        value = driver.to + result[0];
        velocity = result[1];
        if (Math.abs(result[0]) < 0.001 && Math.abs(result[1]) < 0.001) {
          value = driver.to;
          velocity = 0;
          done = true;
        }
      }
      node.value = value;
      node.velocity = velocity;
      if (done) {
        resolveAnimation(run.animation, true);
        node.running = null;
      }
    }
    applyAll();
    if (hasAnimations()) wake();
  }

  function hasAnimations() {
    for (const node of nodes.values()) if (node.running) return true;
    return false;
  }

  function wake() {
    if (frame) return;
    const raf = globalThis.requestAnimationFrame;
    if (typeof raf !== "function") {
      // No frame clock (a test page with no rAF): run the animation to its end.
      let now = 0;
      while (hasAnimations()) {
        now += FRAME / 1000;
        tick(now);
        if (now > 3600) break;
      }
      return;
    }
    frame = raf(function (time) {
      frame = 0;
      tick(time / 1000);
    });
  }

  function applyView(id) {
    const entry = views.get(id);
    if (!entry) return;
    const { el, bindings } = entry;
    const values = [];
    let animatedTransform = false;
    for (const [prop, node] of bindings) {
      const value = valueOf(node);
      if (prop === "opacity") {
        el.style.opacity = String(value);
      } else if (prop === "backgroundColor") {
        el.style.backgroundColor = cssColor(value);
      } else if (prop === "borderColor") {
        if (!el._border) el.style.borderColor = cssColor(value);
        else el._border.style.borderColor = cssColor(value);
      } else if (prop.startsWith("transform:")) {
        animatedTransform = true;
        values[Number(prop.slice(10))] = value;
      }
    }
    if (animatedTransform) {
      const steps = Array.isArray(el._transform) ? el._transform : [];
      const out = [];
      for (let i = 0; i < steps.length; i++) {
        const step = steps[i];
        const kind = Object.keys(step)[0];
        out.push(transformStep(kind, values[i] === undefined ? step[kind] : values[i]));
      }
      el.style.transform = out.join(" ");
    }
  }

  function applyAll() {
    for (const id of views.keys()) applyView(id);
  }

  function bind(id, el, bindings) {
    if (!bindings || bindings.length === 0) {
      views.delete(id);
      clearAnimated(el);
      return;
    }
    views.set(id, { el, bindings });
    clearAnimated(el);
    applyView(id);
  }

  function refresh(id) {
    const entry = views.get(id);
    if (entry) {
      clearAnimated(entry.el);
      applyView(id);
    }
  }

  function unbind(id) {
    const entry = views.get(id);
    if (entry) clearAnimated(entry.el);
    views.delete(id);
  }

  function clearAnimated(el) {
    el.style.opacity = "";
    el.style.backgroundColor = "";
    el.style.borderColor = "";
    if (el._border) el._border.style.borderColor = "";
    el.style.transform = "";
  }

  function makeNodeObject(proto, id, inputs) {
    return makeNode(proto, id, inputs);
  }

  function AnimatedNode() {}
  function AnimatedValue(value) {
    const id = createValue(value === undefined ? 0 : value);
    return makeNodeObject(AnimatedValue.prototype, id, []);
  }
  Object.setPrototypeOf(AnimatedValue.prototype, AnimatedNode.prototype);
  Object.assign(AnimatedNode.prototype, {
    interpolate(config) {
      const id = createInterpolate(this.__av, config || {});
      return makeNodeObject(Object.getPrototypeOf(this), id, [this]);
    },
  });
  Object.assign(AnimatedValue.prototype, {
    set(value) {
      const node = nodes.get(this.__av);
      if (!node) return;
      stopNode(this.__av);
      node.value = Number(value) || 0;
      node.velocity = 0;
      applyAll();
    },
    timing(config) {
      return start(this.__av, timingDriver(config));
    },
    spring(config) {
      return start(this.__av, springDriver(config));
    },
    stop() {
      const value = valueOf(this.__av);
      stopNode(this.__av);
      applyAll();
      return Promise.resolve({ value });
    },
    getValue() {
      return Promise.resolve(valueOf(this.__av));
    },
  });
  AnimatedValue.prototype.get = AnimatedValue.prototype.getValue;
  const Animated = {
    timing(value, config) {
      return value.timing(config);
    },
    spring(value, config) {
      return value.spring(config);
    },
  };

  return {
    api: { createAnimatedValue: AnimatedValue, Animated },
    bind,
    unbind,
    refresh,
    dispose() {
      if (frame && globalThis.cancelAnimationFrame) globalThis.cancelAnimationFrame(frame);
      frame = 0;
      views.clear();
      nodes.clear();
    },
  };
}

function timingDriver(config = {}) {
  const to = Number(config.toValue);
  if (!Number.isFinite(to)) throw new TypeError("animation: toValue must be a number");
  return {
    kind: "timing",
    to,
    duration: Math.max(0, config.duration === undefined ? 300 : Number(config.duration)),
    easing: parseEasing(config.easing === undefined ? "ease-in-out" : config.easing),
    delay: Math.max(0, config.delay === undefined ? 0 : Number(config.delay)),
  };
}

function springDriver(config = {}) {
  const to = Number(config.toValue);
  if (!Number.isFinite(to)) throw new TypeError("animation: toValue must be a number");
  return {
    kind: "spring",
    to,
    stiffness: config.stiffness === undefined ? 170 : Number(config.stiffness),
    damping: config.damping === undefined ? 26 : Number(config.damping),
    mass: config.mass === undefined ? 1 : Number(config.mass),
    velocity: config.velocity === undefined ? null : Number(config.velocity),
  };
}

function parseEasing(value) {
  if (value === undefined || value === "linear") return { kind: "linear" };
  if (value === "ease") return { kind: "bezier", points: [0.25, 0.1, 0.25, 1] };
  if (value === "ease-in") return { kind: "bezier", points: [0.42, 0, 1, 1] };
  if (value === "ease-out") return { kind: "bezier", points: [0, 0, 0.58, 1] };
  if (value === "ease-in-out") return { kind: "bezier", points: [0.42, 0, 0.58, 1] };
  if (Array.isArray(value) && value.length === 4) {
    return { kind: "bezier", points: value.map(Number) };
  }
  if (typeof value === "object" && value && value.steps !== undefined) {
    return {
      kind: "steps",
      count: Math.max(1, Number(value.steps) | 0),
      position: value.position || "end",
    };
  }
  if (typeof value === "string") {
    const bezier = /^cubic-bezier\(([^)]+)\)$/.exec(value);
    if (bezier) return { kind: "bezier", points: bezier[1].split(",").map(Number) };
    const steps = /^steps\(([^)]+)\)$/.exec(value);
    if (steps) {
      const parts = steps[1].split(",").map((s) => s.trim());
      return {
        kind: "steps",
        count: Math.max(1, Number(parts[0]) | 0),
        position: parts[1] || "end",
      };
    }
  }
  throw new TypeError("easing: unsupported value");
}

function ease(easing, p) {
  if (easing.kind === "linear") return p;
  if (easing.kind === "steps") {
    const count = easing.count;
    const none = easing.position === "jump-none";
    const jumps = none ? Math.max(1, count - 1) : count;
    const offset =
      easing.position === "start" ||
      easing.position === "jump-start" ||
      easing.position === "jump-both"
        ? 1
        : 0;
    const step = Math.floor(p * count) + offset;
    return Math.max(0, Math.min(jumps, step)) / jumps;
  }
  const [x1, y1, x2, y2] = easing.points;
  const sample = (t, a, b) => 3 * (1 - t) * (1 - t) * t * a + 3 * (1 - t) * t * t * b + t * t * t;
  const slope = (t, a, b) =>
    3 * (1 - t) * (1 - t) * a + 6 * (1 - t) * t * (b - a) + 3 * t * t * (1 - b);
  let t = p;
  for (let i = 0; i < 8; i++) {
    const error = sample(t, x1, x2) - p;
    if (Math.abs(error) < 1e-6) return sample(t, y1, y2);
    const d = slope(t, x1, x2);
    if (Math.abs(d) < 1e-6) break;
    t -= error / d;
  }
  let lo = 0;
  let hi = 1;
  t = p;
  for (let i = 0; i < 32; i++) {
    const x = sample(t, x1, x2);
    if (Math.abs(x - p) < 1e-6) break;
    if (x < p) lo = t;
    else hi = t;
    t = (lo + hi) / 2;
  }
  return Math.max(0, Math.min(1, sample(t, y1, y2)));
}

function spring(x0, v0, k, c, m, t) {
  m = Math.max(1e-6, m);
  k = Math.max(0, k);
  c = Math.max(0, c);
  const w0 = Math.sqrt(k / m);
  if (w0 === 0) return [x0 + v0 * t, v0];
  const zeta = c / (2 * Math.sqrt(k * m));
  if (zeta < 1) {
    const wd = w0 * Math.sqrt(1 - zeta * zeta);
    if (wd === 0) return [x0, v0];
    const a = x0;
    const b = (v0 + zeta * w0 * x0) / wd;
    const e = Math.exp(-zeta * w0 * t);
    const s = Math.sin(wd * t);
    const co = Math.cos(wd * t);
    return [
      e * (a * co + b * s),
      e * ((b * wd - zeta * w0 * a) * co - (a * wd + zeta * w0 * b) * s),
    ];
  }
  if (Math.abs(zeta - 1) < 1e-9) {
    const e = Math.exp(-w0 * t);
    const b = v0 + w0 * x0;
    return [e * (x0 + b * t), e * (b - w0 * (x0 + b * t))];
  }
  const wd = w0 * Math.sqrt(zeta * zeta - 1);
  const a = x0;
  const b = (v0 + zeta * w0 * x0) / wd;
  const e = Math.exp(-zeta * w0 * t);
  const s = Math.sinh(wd * t);
  const co = Math.cosh(wd * t);
  return [e * (a * co + b * s), e * ((b * wd - zeta * w0 * a) * co + (a * wd - zeta * w0 * b) * s)];
}

function parseColor(input) {
  if (typeof input === "number") return input >>> 0;
  const text = String(input).trim().toLowerCase();
  if (text === "transparent") return 0;
  const named = {
    black: 0x000000ff,
    white: 0xffffffff,
    red: 0xff0000ff,
    green: 0x008000ff,
    blue: 0x0000ffff,
  };
  if (Object.prototype.hasOwnProperty.call(named, text)) return named[text];
  if (text[0] === "#") {
    let hex = text.slice(1);
    if (hex.length === 3 || hex.length === 4) {
      hex = hex
        .split("")
        .map((c) => c + c)
        .join("");
    }
    if (hex.length === 6) hex += "ff";
    if (hex.length !== 8 || /[^0-9a-f]/.test(hex))
      throw new TypeError("interpolate: invalid color " + input);
    return parseInt(hex, 16) >>> 0;
  }
  const m = /^rgba?\(([^)]+)\)$/.exec(text);
  if (!m) throw new TypeError("interpolate: invalid color " + input);
  const parts = m[1]
    .split(/[ ,/]+/)
    .filter(Boolean)
    .map(Number);
  if (parts.length < 3 || parts.length > 4 || parts.some((n) => !Number.isFinite(n))) {
    throw new TypeError("interpolate: invalid color " + input);
  }
  const alpha = parts.length === 4 ? Math.round(parts[3] * 255) : 255;
  const c = parts.slice(0, 3).map((n) => Math.max(0, Math.min(255, Math.round(n))));
  return ((c[0] << 24) | (c[1] << 16) | (c[2] << 8) | (alpha & 255)) >>> 0;
}

function lerpColor(a, b, t) {
  let out = 0;
  for (const shift of [24, 16, 8, 0]) {
    const ca = (a >>> shift) & 0xff;
    const cb = (b >>> shift) & 0xff;
    const c = Math.round(ca + (cb - ca) * t) & 0xff;
    out = (out | (c << shift)) >>> 0;
  }
  return out;
}

function cssColor(value) {
  const c = value >>> 0;
  const r = (c >>> 24) & 0xff;
  const g = (c >>> 16) & 0xff;
  const b = (c >>> 8) & 0xff;
  const a = (c & 0xff) / 255;
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

function transformStep(kind, value) {
  if (kind === "translateX") return `translateX(${value}px)`;
  if (kind === "translateY") return `translateY(${value}px)`;
  if (kind === "scale") return `scale(${value})`;
  if (kind === "scaleX") return `scaleX(${value})`;
  if (kind === "scaleY") return `scaleY(${value})`;
  if (kind === "rotate") return `rotate(${value}deg)`;
  if (kind === "rotateX") return `rotateX(${value}deg)`;
  if (kind === "rotateY") return `rotateY(${value}deg)`;
  if (kind === "perspective") return `perspective(${value}px)`;
  return "";
}
