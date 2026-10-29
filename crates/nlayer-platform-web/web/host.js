// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// The page side of `nlayer test --target web`. Steps name a view with the
// same selector as the headless runner. A press, a key, and a scroll go
// through this document; nothing here calls the engine.

/**
 * Installs `globalThis.__nlayerHost` for the page `player` mounted in `root`.
 * @param {import("./nlayer.js").Player} player
 * @param {Element} root
 */
export function installHost(player, root) {
  const host = {
    texts() {
      return textsOf(player);
    },
    inputs() {
      return inputsOf(player);
    },
    tree() {
      return nodeJson(root, null, root);
    },
    press(selector) {
      const target = one(player, root, selector);
      visible(root, target, selector);
      if (!canPress(target)) throw new Error(`\`${selector}\` does not take a press`);
      click(target);
      return null;
    },
    type(selector, text) {
      const target = one(player, root, selector);
      visible(root, target, selector);
      const field = inputField(target);
      if (!field) throw new Error(`\`${selector}\` is not an input`);
      if (field.disabled) throw new Error(`\`${selector}\` is disabled`);
      field.focus?.();
      for (const ch of String(text)) insertChar(field, ch);
      return null;
    },
    scroll(selector, distance) {
      const target = one(player, root, selector);
      visible(root, target, selector);
      const el = scrollOf(target);
      if (!el) throw new Error(`\`${selector}\` is not in a scroll view`);
      moveScroll(el, distance);
      return null;
    },
    query(selector) {
      try {
        const target = resolve(player, selector);
        if (!isVisible(root, target)) return { state: "wait" };
        return { state: "ready" };
      } catch (error) {
        const message = error && error.message ? String(error.message) : String(error);
        if (message.includes("matches nothing")) return { state: "wait" };
        return { state: "fail", error: message };
      }
    },
  };
  globalThis.__nlayerHost = host;
  return host;
}

function textsOf(player) {
  const out = [];
  for (const el of byId(player)) {
    const text = textOf(el);
    if (text != null) out.push(text);
  }
  return out;
}

function inputsOf(player) {
  const out = [];
  for (const el of byId(player)) {
    if (el._field) out.push(el._field.value ?? "");
  }
  return out;
}

function byId(player) {
  return [...player.views.values()].sort((a, b) => a._id - b._id);
}

function one(player, root, selector) {
  return resolve(player, selector);
}

function resolve(player, source) {
  const selector = parse(source);
  let scope = null;
  for (const part of selector.parts) {
    const hits = collect(player, part.matcher, scope);
    scope = pick(selector.source, hits, part.nth);
  }
  const hits = scope ?? [];
  if (hits.length === 1) return hits[0];
  if (hits.length === 0) throw new Error(`\`${selector.source}\` matches nothing`);
  throw new Error(
    `\`${selector.source}\` matches ${hits.length} views: say which (\`${selector.source} >> nth=0\`)`,
  );
}

function parse(source) {
  const parts = [];
  for (const raw of String(source).split(" >> ")) {
    const piece = raw.trim();
    if (piece.startsWith("nth=")) {
      const n = piece.slice(4);
      if (!/^\d+$/.test(n)) {
        throw new Error(`\`${source}\`: \`${piece}\`: not an index`);
      }
      const last = parts.at(-1);
      if (!last) throw new Error(`\`${source}\`: \`nth=\` follows what it picks from`);
      last.nth = Number(n);
      continue;
    }
    if (piece === "") throw new Error(`\`${source}\`: an empty selector`);
    parts.push({ matcher: matcher(piece), nth: null });
  }
  if (parts.length === 0) throw new Error(`\`${source}\`: an empty selector`);
  return { source: String(source), parts };
}

function matcher(piece) {
  const split = piece.indexOf(":");
  if (split > 0) {
    const kind = piece.slice(0, split);
    const value = piece.slice(split + 1);
    if (kind === "id") return { kind: "id", value };
    if (kind === "text") return { kind: "text", value: value.trim() };
  }
  return { kind: "any", value: piece };
}

function collect(player, matcher, scope) {
  if (matcher.kind === "any") {
    const ids = collect(player, { kind: "id", value: matcher.value }, scope);
    return ids.length > 0 ? ids : collect(player, { kind: "text", value: matcher.value }, scope);
  }
  const out = [];
  if (scope) {
    for (const hit of scope) {
      if (hit.span) continue;
      const own = [];
      pushMatches(hit.el, matcher, own);
      for (const item of own) {
        if (item.span) out.push(item);
      }
      for (const child of childViews(hit.el)) walk(child, matcher, out);
    }
    return out;
  }
  const root = [...player.views.values()].find((el) => el._id === 1);
  if (root) walk(root, matcher, out);
  return out;
}

function walk(el, matcher, out) {
  pushMatches(el, matcher, out);
  for (const child of childViews(el)) walk(child, matcher, out);
}

function pushMatches(el, matcher, out) {
  if (matcher.kind === "id") {
    if (el._testId && el._testId === matcher.value) out.push(viewTarget(el));
    for (const span of el._spanIds ?? []) {
      if (span[0] === matcher.value) out.push(spanTarget(el, span[1], span[2]));
    }
    return;
  }
  const text = textOf(el);
  if (textEq(text, matcher.value) || inputEq(el, matcher.value)) out.push(viewTarget(el));
  if (text == null) return;
  for (const span of el._spanIds ?? []) {
    if (textEq(text.slice(span[1], span[2]), matcher.value))
      out.push(spanTarget(el, span[1], span[2]));
  }
}

function pick(source, hits, nth) {
  if (nth != null) return hits[nth] ? [hits[nth]] : [];
  if (hits.length > 1) {
    throw new Error(
      `\`${source}\` matches ${hits.length} views: say which (\`${source} >> nth=0\`)`,
    );
  }
  return hits;
}

function viewTarget(el) {
  return { el, span: null };
}

function spanTarget(el, start, end) {
  const link = (el._links ?? []).find((item) => item[0] === start && item[1] === end);
  return { el, span: { start, end, node: link ? link[2] : null } };
}

function textEq(text, want) {
  return text != null && text.trim() === want.trim();
}

function inputEq(el, want) {
  const field = el._field;
  if (!field) return false;
  if (textEq(field.value ?? "", want)) return true;
  return (field.value ?? "") === "" && textEq(field.placeholder ?? "", want);
}

function textOf(el) {
  const box = el._text;
  if (!box) return null;
  let text = "";
  for (const span of box.children ?? []) text += span.textContent ?? "";
  return text;
}

function childViews(el) {
  return [...(el.children ?? [])].filter((child) => child && child._kind);
}

function parentView(el) {
  let cur = el.parentNode;
  while (cur) {
    if (cur._kind) return cur;
    cur = cur.parentNode;
  }
  return null;
}

function visible(root, target, selector) {
  if (!isVisible(root, target)) throw new Error(`\`${selector}\` is not in view`);
}

function isVisible(root, target) {
  const box = clientBox(target);
  if (box.width <= 0 || box.height <= 0) return false;
  if (hidden(target.el)) return false;
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  const view = elementBox(root);
  if (cx < view.x || cy < view.y || cx > view.x + view.width || cy > view.y + view.height)
    return false;
  let el = parentView(target.el);
  while (el) {
    if (hidden(el)) return false;
    if (el._kind === "scroll") {
      const scroll = elementBox(el);
      if (
        cx < scroll.x ||
        cy < scroll.y ||
        cx > scroll.x + scroll.width ||
        cy > scroll.y + scroll.height
      ) {
        return false;
      }
    }
    el = parentView(el);
  }
  return true;
}

function hidden(el) {
  for (let cur = el; cur; cur = parentView(cur)) {
    if (opacity(cur) <= 0) return true;
    const style = cur.ownerDocument?.defaultView?.getComputedStyle?.(cur);
    if (style && (style.visibility === "hidden" || style.display === "none")) return true;
  }
  return false;
}

function opacity(el) {
  if (typeof el._opacity === "number") return el._opacity;
  const style = el.ownerDocument?.defaultView?.getComputedStyle?.(el);
  const value = style ? Number(style.opacity) : 1;
  return Number.isNaN(value) ? 1 : value;
}

function clientBox(target) {
  if (target.span) {
    const ranged = rangeBox(target.el, target.span.start, target.span.end);
    if (ranged) return ranged;
  }
  return elementBox(target.el);
}

function elementBox(el) {
  if (typeof el.getBoundingClientRect === "function") {
    const rect = el.getBoundingClientRect();
    if (rect && (rect.width > 0 || rect.height > 0)) {
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    }
  }
  const root = rootOf(el);
  const origin = visualOrigin(el, root);
  const frame = layoutFrame(el, root);
  return { x: origin.x, y: origin.y, width: frame.width, height: frame.height };
}

function rootOf(el) {
  let cur = el;
  while (parentView(cur)) cur = parentView(cur);
  return cur;
}

function visualOrigin(el, root) {
  if (el === root) return { x: 0, y: 0 };
  const parent = parentView(el) ?? root;
  const origin = visualOrigin(parent, root);
  const frame = layoutFrame(el, root);
  let x = origin.x + frame.x;
  let y = origin.y + frame.y;
  if (parent._kind === "scroll") {
    x -= parent.scrollLeft || 0;
    y -= parent.scrollTop || 0;
  }
  return { x, y };
}

function layoutFrame(el, root) {
  if (el === root && !el._frame) {
    const pin = el._pin;
    return {
      x: 0,
      y: 0,
      width: el.clientWidth || pin?.width || 0,
      height: el.clientHeight || pin?.height || 0,
    };
  }
  const frame = el._frame ?? [0, 0, 0, 0];
  return { x: frame[0], y: frame[1], width: frame[2], height: frame[3] };
}

function rangeBox(el, start, end) {
  const box = el._text;
  const doc = el.ownerDocument;
  if (!box || typeof doc?.createRange !== "function") return null;
  const from = pointIn(box, start);
  const to = pointIn(box, end);
  if (!from || !to) return null;
  const range = doc.createRange();
  range.setStart(from.node, from.offset);
  range.setEnd(to.node, to.offset);
  const rect = range.getBoundingClientRect();
  if (!rect || (rect.width === 0 && rect.height === 0)) return null;
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
}

function pointIn(root, index) {
  let left = index;
  let found = null;
  const walk = (node) => {
    if (node.nodeType === 3) {
      const size = node.textContent.length;
      if (left <= size) {
        found = { node, offset: left };
        return true;
      }
      left -= size;
      return false;
    }
    for (const child of node.childNodes ?? []) {
      if (walk(child)) return true;
    }
    return false;
  };
  return walk(root) ? found : null;
}

function canPress(target) {
  if (target.span?.node != null) return true;
  for (let el = target.el; el; el = parentView(el)) {
    if (el._events?.has("onPress")) return true;
  }
  return false;
}

function click(target) {
  const el = target.el;
  const box = clientBox(target);
  const clientX = box.x + box.width / 2;
  const clientY = box.y + box.height / 2;
  if (typeof el.dispatch === "function" && typeof el.dispatchEvent !== "function") {
    const extra = target.span?.node != null ? { _utf16: target.span.start } : { clientX, clientY };
    el.dispatch("click", extra);
    return;
  }
  const doc = el.ownerDocument;
  const hit = doc.elementFromPoint?.(clientX, clientY) ?? el;
  hit.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX, clientY }));
}

function inputField(target) {
  if (target.span || target.el._kind !== "input") return null;
  return target.el._field ?? null;
}

function insertChar(field, ch) {
  const doc = field.ownerDocument;
  field.focus?.();
  const before = field.value ?? "";
  let inserted = false;
  try {
    inserted = doc?.execCommand?.("insertText", false, ch) === true && field.value !== before;
  } catch {
    inserted = false;
  }
  if (inserted) return;
  field.value = before + ch;
  if (typeof field.dispatch === "function" && typeof field.dispatchEvent !== "function") {
    field.dispatch("input");
    return;
  }
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

function scrollOf(target) {
  for (let el = target.el; el; el = parentView(el)) {
    if (el._kind === "scroll") return el;
  }
  return null;
}

function moveScroll(el, distance) {
  const horizontal = !!el._horizontal;
  const content = spacerSize(el);
  const span = Math.max(
    0,
    horizontal ? content.width - boxSize(el).width : content.height - boxSize(el).height,
  );
  const current = horizontal ? el.scrollLeft || 0 : el.scrollTop || 0;
  const next = Math.min(span, Math.max(0, current + distance));
  const native = typeof el.dispatchEvent === "function";
  if (horizontal) el.scrollLeft = next;
  else el.scrollTop = next;
  if (!native && typeof el.dispatch === "function") el.dispatch("scroll");
}

function boxSize(el) {
  const frame = el._frame;
  if (frame) return { width: frame[2], height: frame[3] };
  return { width: el.clientWidth || 0, height: el.clientHeight || 0 };
}

function spacerSize(el) {
  const spacer = el._spacer;
  if (!spacer) return boxSize(el);
  return {
    width: px(spacer.style?.width) || boxSize(el).width,
    height: px(spacer.style?.height) || boxSize(el).height,
  };
}

function px(value) {
  const n = parseFloat(value);
  return Number.isNaN(n) ? 0 : n;
}

function nodeJson(el, parent, root) {
  const frame = parent
    ? layoutFrame(el, root)
    : { x: 0, y: 0, width: elementBox(el).width, height: elementBox(el).height };
  const node = {
    kind: el._kind,
    frame: { x: frame.x, y: frame.y, width: frame.width, height: frame.height },
    children: childViews(el).map((child) => nodeJson(child, el, root)),
  };
  const text = textOf(el);
  if (text != null) node.text = text;
  if (el._testId) node.testID = el._testId;
  const spans = spanJson(el);
  if (spans) node.spans = spans;
  const field = el._field;
  if (field) {
    node.value = field.value ?? "";
    node.placeholder = field.placeholder ?? "";
    node.secure = !!field._secure;
    node.keyboard = field._keyboard || "text";
    node.disabled = !!field.disabled;
    node.focused = field.ownerDocument?.activeElement === field;
  }
  if (el._kind === "image" && el._img) {
    node.src =
      el._img._uri != null ? el._img._uri : (el._img.getAttribute?.("src") ?? el._img.src ?? "");
    node.fit = fitOf(el._img);
  }
  if (el._kind === "scroll") {
    node.horizontal = !!el._horizontal;
    node.offset = { x: el.scrollLeft || 0, y: el.scrollTop || 0 };
    const content = spacerSize(el);
    node.content = { width: content.width, height: content.height };
  }
  if (el._transform) node.transform = el._transform;
  return node;
}

function spanJson(el) {
  const box = el._text;
  if (!box) return null;
  const spans = [];
  for (const span of box.children ?? []) {
    const item = { text: span.textContent ?? "" };
    const style = span.ownerDocument?.defaultView?.getComputedStyle?.(span);
    if (style) {
      const size = parseFloat(style.fontSize);
      if (!Number.isNaN(size)) item.fontSize = size;
      const weight = parseInt(style.fontWeight, 10);
      if (!Number.isNaN(weight)) item.fontWeight = weight;
      item.italic = style.fontStyle === "italic";
      if (style.color) item.color = hexColor(style.color);
    }
    spans.push(item);
  }
  return spans.length ? spans : null;
}

/** A CSS color as `#rrggbb` or `#rrggbbaa`. `rgb()` / `rgba()` convert; anything else is kept. */
function hexColor(value) {
  const raw = String(value).trim();
  if (raw.startsWith("#")) {
    const hex = raw.toLowerCase();
    return hex.length === 9 && hex.endsWith("ff") ? hex.slice(0, 7) : hex;
  }
  const match = raw.match(
    /^rgba?\(\s*([0-9.]+)[,\s]+([0-9.]+)[,\s]+([0-9.]+)(?:(?:[,\s/]+)([0-9.]+%?))?\s*\)$/i,
  );
  if (!match) return raw;
  const channel = (n) => {
    const v = Math.round(Number(n));
    const c = v < 0 ? 0 : v > 255 ? 255 : v;
    return c.toString(16).padStart(2, "0");
  };
  const rgb = `${channel(match[1])}${channel(match[2])}${channel(match[3])}`;
  if (match[4] == null) return `#${rgb}`;
  const spec = match[4];
  const amount = Number(spec.endsWith("%") ? spec.slice(0, -1) : spec);
  const alpha = spec.endsWith("%") ? (amount / 100) * 255 : amount <= 1 ? amount * 255 : amount;
  const a = Math.round(alpha);
  return a >= 255 ? `#${rgb}` : `#${rgb}${channel(a)}`;
}

function fitOf(img) {
  const name = img.className || "";
  if (name.includes("nl-fit-contain")) return "contain";
  if (name.includes("nl-fit-fill")) return "fill";
  if (name.includes("nl-fit-scale-down")) return "scale-down";
  if (name.includes("nl-fit-none")) return "none";
  return "cover";
}
