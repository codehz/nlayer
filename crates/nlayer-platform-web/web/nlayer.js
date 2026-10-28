// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// The page's side of a mount batch (`Encoder` in this crate). Extracted
// from nsh's DOM mounter and cut to views, text, images, one-axis scroll
// views and single-line inputs: absolute frames, generated classes, a
// border overlay, a scroll spacer and a field whose value ignores a stale
// edit count. `activeStyle`, `hoverStyle` and `focusStyle` are rules the
// page applies (`rule`) while the pointer state holds (`st`).

/**
 * The style classes of the document. A class named after its CSS
 * (`nl-` and 8 hex digits, from the engine or `cssClass`) is one rule;
 * a view's own rule (`i<id>…`, `nl-p-` in front) is replaced as it changes.
 * Name → its CSS text.
 */
const CLASSES = { sheets: {}, rules: new Map(), texts: new Map() };

/** The sheet of generated classes, or of the views' own rules. */
function classSheet(doc, id) {
  if (!CLASSES.sheets[id]) {
    const style = doc.createElement("style");
    style.id = id;
    (doc.head ?? doc.documentElement).append(style);
    CLASSES.sheets[id] = style.sheet ?? style;
  }
  return CLASSES.sheets[id];
}

/**
 * Adds (or, for a view's own rule, replaces) class `name` with `text`.
 * The class is doubled so it wins over the base rules.
 */
function defineClass(doc, name, text, replace) {
  const rule = CLASSES.rules.get(name);
  if (rule) {
    if (replace) rule.style.cssText = text;
    return;
  }
  const sheet = classSheet(doc, replace ? "nl-own" : "nl-classes");
  const selector = `.${name}.${name}`;
  if (typeof sheet.insertRule === "function") {
    const i = sheet.insertRule(`${selector}{${text}}`, sheet.cssRules.length);
    CLASSES.rules.set(name, sheet.cssRules[i]);
  } else {
    const rule = { style: { cssText: text }, cssText: `${selector}{${text}}` };
    sheet.cssRules = sheet.cssRules ?? [];
    sheet.cssRules.push(rule);
    sheet.textContent = (sheet.textContent ?? "") + rule.cssText;
    CLASSES.rules.set(name, rule);
  }
  CLASSES.texts.set(name, text);
}

/** Puts `el` in class `name` in place of its previous one of `slot`. */
function swapClass(el, slot, name) {
  const old = el[slot];
  if (old === name) return;
  if (old) el.classList.remove(old);
  if (name) el.classList.add(name);
  el[slot] = name;
}

/**
 * A state rule. `selector` already includes the class, the pseudo-class
 * and whatever it beats. An empty `text` clears a rule the page already
 * has and does not add one.
 */
function defineSelector(doc, selector, text) {
  const rule = CLASSES.rules.get(selector);
  if (!text) {
    if (rule) rule.style.cssText = "";
    return;
  }
  if (rule) {
    rule.style.cssText = text;
    return;
  }
  const sheet = classSheet(doc, "nl-states");
  if (typeof sheet.insertRule === "function") {
    const i = sheet.insertRule(`${selector}{${text}}`, sheet.cssRules.length);
    CLASSES.rules.set(selector, sheet.cssRules[i]);
  } else {
    const entry = { style: { cssText: text }, cssText: `${selector}{${text}}` };
    sheet.cssRules = sheet.cssRules ?? [];
    sheet.cssRules.push(entry);
    sheet.textContent = (sheet.textContent ?? "") + entry.cssText;
    CLASSES.rules.set(selector, entry);
  }
}

/** Installs the base rules once. */
export function installBase(doc, css) {
  if (doc.getElementById?.("nl-base")) return;
  const style = doc.createElement("style");
  style.id = "nl-base";
  style.textContent = css;
  (doc.head ?? doc.documentElement).append(style);
}

/**
 * One surface. `root` is the platform's root view (node 1). `host` hears
 * what the page must send back to the engine: `press(id)`,
 * `change(id, text)`, `scroll(id, name, x, y, contentWidth, contentHeight, width, height)`.
 * `name` is the event (`onScroll`, `onScrollEnd`, `onScrollDragStart`,
 * `onScrollDragEnd`, `onMomentumStart`, `onMomentumEnd`). Lengths are CSS
 * pixels. `assetBase` is prepended to an `asset:/` URI.
 */
export class Player {
  constructor(root, host = {}, options = {}) {
    this.root = root;
    this.host = host;
    this.assetBase = options.assetBase ?? "";
    this.doc = root.ownerDocument ?? globalThis.document;
    this.views = new Map();
    this.pointers = new Map();
    this.touchDrags = new Set();
    this.holding = 0;
    this.onTouchEnd = (event) => this.touchEnded(event);
    root._id = 1;
    root._kind = "view";
    root.classList.add("nl-v", "nl-root");
    this.views.set(1, root);
    root.addEventListener("pointerdown", (event) => this.pointerDown(event), true);
    root.addEventListener("pointercancel", (event) => this.pointerCancel(event), true);
    root.addEventListener("touchend", this.onTouchEnd);
    root.addEventListener("touchcancel", this.onTouchEnd);
    const view = this.doc.defaultView;
    if (view && view.addEventListener && view !== root) {
      view.addEventListener("touchend", this.onTouchEnd);
      view.addEventListener("touchcancel", this.onTouchEnd);
    }
    if (options.baseCSS != null) installBase(this.doc, options.baseCSS);
  }

  apply(batch) {
    for (const item of batch) {
      const handler = this[`op_${item[0]}`];
      if (handler) handler.call(this, item);
    }
  }

  view(id) {
    return this.views.get(id);
  }

  asset(uri) {
    if (!uri) return uri;
    if (uri.startsWith("asset:/")) {
      const path = uri.slice("asset:/".length).replace(/^\/+/, "");
      return this.assetBase + path;
    }
    return uri;
  }

  /** A view's own rule carries the page prefix; a shared class does not. */
  styleClass(name) {
    if (!name) return null;
    return name.charCodeAt(0) === 105 ? `nl-p-${name}` : name;
  }

  op_css([, name, text]) {
    const own = name.charCodeAt(0) === 105;
    defineClass(this.doc, own ? `nl-p-${name}` : name, text, own);
  }

  op_c([, id, kind]) {
    const el = this.doc.createElement("div");
    el._id = id;
    el._kind = kind;
    el._events = new Set();
    el.className = kind === "scroll" ? "nl-v nl-scroll" : "nl-v";
    if (kind === "image") {
      const img = this.doc.createElement("img");
      img.draggable = false;
      img.className = "nl-fill nl-fit-cover";
      el.append(img);
      el._img = img;
    }
    if (kind === "scroll") this.wireScroll(el);
    el.addEventListener("click", (event) => {
      const link = linkAt(el, event);
      if (link != null) {
        event.stopPropagation?.();
        this.host.press?.(link);
        return;
      }
      if (!el._events.has("onPress")) return;
      event.stopPropagation?.();
      this.host.press?.(id);
    });
    this.views.set(id, el);
  }

  op_rule([, name, weight, pseudo, tail, text]) {
    const cls = this.styleClass(name);
    if (!cls) return;
    const selector = `.${cls}`.repeat(weight) + pseudo + ":not(.nl-disabled)" + (tail ?? "");
    defineSelector(this.doc, selector, text ?? "");
  }

  op_st([, id, which, name]) {
    const el = this.view(id);
    if (!el) return;
    const slot = which === "active" ? "_active" : which === "focus" ? "_focus" : "_hover";
    swapClass(el, slot, this.styleClass(name));
    if (which === "focus") {
      if (name && el._kind !== "input") el.tabIndex = 0;
      else {
        el.tabIndex = undefined;
        el.removeAttribute?.("tabindex");
      }
    }
  }

  op_i([, parentId, childId, beforeId]) {
    const parent = this.view(parentId);
    const child = this.view(childId);
    if (!parent || !child) return;
    const before = beforeId == null ? null : this.view(beforeId);
    parent.insertBefore(
      child,
      before && before.parentNode === parent ? before : (parent._spacer ?? null),
    );
    this.raiseBorder(parent);
  }

  op_r([, id]) {
    this.view(id)?.remove();
  }

  op_d([, id]) {
    const el = this.view(id);
    if (!el) return;
    this.views.delete(id);
    el.remove();
  }

  op_f([, id, x, y, w, h]) {
    const el = this.view(id);
    if (!el) return;
    el._frame = [x, y, w, h];
    const s = el.style;
    s.left = `${x}px`;
    s.top = `${y}px`;
    s.width = `${w}px`;
    s.height = `${h}px`;
  }

  op_s([, id, css, border]) {
    const el = this.view(id);
    if (!el) return;
    swapClass(el, "_class", this.styleClass(css));
    if (border) {
      if (!el._border) el._border = this.doc.createElement("div");
      el._border.className = `nl-border ${this.styleClass(border)}`;
      el.append(el._border);
    } else if (el._border) {
      el._border.remove();
      el._border = null;
    }
  }

  raiseBorder(el) {
    if (el._border) el.append(el._border);
  }

  op_t([, id, x, y, w, h, align, rtl, runs, spanIds, links]) {
    const el = this.view(id);
    if (!el) return;
    let box = el._text;
    if (!box) {
      box = this.doc.createElement("div");
      box.className = "nl-text";
      el._text = box;
      el.insertBefore(box, el.firstChild);
    }
    const s = box.style;
    s.left = `${x}px`;
    s.top = `${y}px`;
    s.width = `${w}px`;
    s.height = `${h}px`;
    s.textAlign = align || "";
    s.direction = rtl ? "rtl" : "";
    box.replaceChildren();
    for (const [text, name] of runs) {
      const span = this.doc.createElement("span");
      const cls = this.styleClass(name);
      if (cls) span.className = cls;
      span.textContent = text;
      box.append(span);
    }
    el._spanIds = spanIds || [];
    el._links = links || [];
  }

  op_e([, id, events]) {
    const el = this.view(id);
    if (!el) return;
    el._events = new Set(events);
  }

  op_tid([, id, name]) {
    const el = this.view(id);
    if (el) el._testId = name || "";
  }

  op_img([, id, uri, mode]) {
    const img = this.view(id)?._img;
    if (!img) return;
    img.className = `nl-fill nl-fit-${mode || "cover"}`;
    // The page loads `asset()`'s URL. The test tree reports the engine URI.
    img._uri = uri ?? "";
    const src = this.asset(uri ?? "");
    if (!src) img.removeAttribute("src");
    else img.src = src;
  }

  /**
   * Touch scrolling is the browser's (nsh's `touchScroll` / `scrolled` /
   * `scrollEnded`, cut to no refresh, zoom, snap or custom bar). A
   * `pointercancel` starts the drag, `touchend` ends it, a scroll within
   * 150 ms is the glide, and `scrollend` or 200 ms of quiet ends it. A
   * glide caught by a new drag ends with `onMomentumEnd` before
   * `onScrollDragStart`, as UIKit does. Wheel, keys and the bar are not
   * drags. An instant command is not an end. `onScroll` is at most once
   * per frame. Web does not bounce, so the offset is not negative.
   */
  wireScroll(el) {
    el._spacer = this.doc.createElement("div");
    el._spacer.className = "nl-spacer";
    el.append(el._spacer);
    el._horizontal = false;
    el.addEventListener("scroll", () => this.scrolled(el), { passive: true });
    el.addEventListener("scrollend", () => this.scrollEnded(el));
    // The user takes over an instant command (`noteInstant`).
    const takeOver = () => {
      el._swallowEnd = false;
    };
    for (const type of ["wheel", "pointerdown", "touchstart", "keydown"]) {
      el.addEventListener(type, takeOver, { passive: true });
    }
  }

  now() {
    return globalThis.performance?.now?.() ?? Date.now();
  }

  /** Scroll views from `node` out to the root, innermost first. */
  scrollChain(node) {
    const chain = [];
    for (let el = node; el; el = el.parentNode) {
      if (el._kind === "scroll") chain.push(el);
    }
    return chain;
  }

  pointerDown(event) {
    if (event.pointerType === "mouse" || event.button > 0) return;
    const scrolls = this.scrollChain(event.target);
    if (!scrolls.length) return;
    this.holding++;
    this.pointers.set(event.pointerId ?? 0, {
      x: event.clientX || 0,
      y: event.clientY || 0,
      scrolls,
    });
  }

  pointerCancel(event) {
    if (event.pointerType === "mouse") return;
    const id = event.pointerId ?? 0;
    const tracked = this.pointers.get(id);
    this.pointers.delete(id);
    const scrolls = tracked?.scrolls ?? this.scrollChain(event.target);
    if (!scrolls.length) return;
    const dx = (event.clientX || 0) - (tracked?.x || 0);
    const dy = (event.clientY || 0) - (tracked?.y || 0);
    const horizontal = Math.abs(dx) > Math.abs(dy);
    const el = scrolls.find((view) => !!view._horizontal === horizontal) ?? scrolls[0];
    this.beginDrag(el);
  }

  beginDrag(el) {
    if (!el || el._touchDrag) return;
    el._touchDrag = true;
    el._released = 0;
    el._settled = false;
    clearTimeout(el._restTimer);
    el._restTimer = 0;
    this.touchDrags.add(el);
    if (el._momentum) {
      el._momentum = false;
      this.scrollEvent(el, "onMomentumEnd");
    }
    this.scrollEvent(el, "onScrollDragStart");
  }

  touchEnded(event) {
    if (event.touches?.length) return;
    if (!this.holding && !this.touchDrags.size) return;
    this.holding = 0;
    for (const el of this.touchDrags) this.endDrag(el);
    this.touchDrags.clear();
    this.pointers.clear();
  }

  endDrag(el) {
    if (!el._touchDrag) return;
    el._touchDrag = false;
    el._released = this.now();
    this.scrollEvent(el, "onScrollDragEnd");
    this.armRest(el);
  }

  scrolled(el) {
    el._settled = false;
    // Still moving after the finger left: a glide.
    if (el._released && !el._momentum && this.now() - el._released < 150) {
      el._momentum = true;
      el._released = 0;
      this.scrollEvent(el, "onMomentumStart");
    }
    if (el._events.has("onScroll")) this.queueScroll(el);
    if (!el._touchDrag && !this.holding && !el._swallowEnd) this.armRest(el);
  }

  /** The latest offset, once per frame when the page has a frame clock. */
  queueScroll(el) {
    if (el._scrollQueued) return;
    el._scrollQueued = true;
    const send = () => {
      el._scrollQueued = false;
      if (this.views.get(el._id) !== el) return;
      this.scrollEvent(el, "onScroll");
    };
    const raf = this.doc.defaultView?.requestAnimationFrame ?? globalThis.requestAnimationFrame;
    if (typeof raf === "function") raf(send);
    else send();
  }

  armRest(el) {
    clearTimeout(el._restTimer);
    el._restTimer = setTimeout(() => {
      el._restTimer = 0;
      this.scrollEnded(el);
    }, 200);
  }

  /** The browser's scrolling stopped, unless a finger still holds it. */
  scrollEnded(el) {
    clearTimeout(el._restTimer);
    el._restTimer = 0;
    if (el._touchDrag || this.holding) return;
    // An instant command moved the view. Its `scrollend` is not the end.
    if (el._swallowEnd) {
      el._swallowEnd = false;
      clearTimeout(el._instantTimer);
      return;
    }
    if (el._settled) return;
    el._settled = true;
    el._released = 0;
    if (el._momentum) {
      el._momentum = false;
      this.scrollEvent(el, "onMomentumEnd");
    }
    this.scrollEvent(el, "onScrollEnd");
  }

  scrollEvent(el, name) {
    if (!el._events?.has(name)) return;
    this.host.scroll?.(
      el._id,
      name,
      el.scrollLeft,
      el.scrollTop,
      el.scrollWidth,
      el.scrollHeight,
      el.clientWidth,
      el.clientHeight,
    );
  }

  /** `op_sb` and an instant `scrollTo` move the view without ending a scroll. */
  noteInstant(el) {
    el._swallowEnd = true;
    clearTimeout(el._instantTimer);
    el._instantTimer = setTimeout(() => {
      el._swallowEnd = false;
    }, 200);
  }

  op_sc([, id, horizontal]) {
    const el = this.view(id);
    if (!el) return;
    el._horizontal = horizontal;
    swapClass(el, "_overflow", horizontal ? "nl-ox" : "nl-oy");
  }

  op_scs([, id, w, h]) {
    const el = this.view(id);
    if (!el?._spacer) return;
    el._spacer.style.width = `${w}px`;
    el._spacer.style.height = `${h}px`;
  }

  op_sb([, id, dx, dy]) {
    const el = this.view(id);
    if (!el) return;
    this.noteInstant(el);
    el.scrollLeft += dx;
    el.scrollTop += dy;
  }

  op_in([, id, css, config]) {
    const el = this.view(id);
    if (!el) return;
    let field = el._field;
    if (!field) {
      field = this.doc.createElement("input");
      field.className = "nl-field nl-inset";
      field._count = 0;
      field.addEventListener("input", () => {
        if (field.disabled) return;
        if (field.value === field._known) return;
        field._known = field.value;
        field._count++;
        this.host.change?.(id, field.value);
      });
      el._field = field;
      el.append(field);
      if (el._content) this.placeContent(el);
    }
    swapClass(field, "_class", this.styleClass(css));
    field.placeholder = config.placeholder;
    field.type = config.secure
      ? "password"
      : config.keyboard === "email"
        ? "email"
        : config.keyboard === "phone"
          ? "tel"
          : "text";
    field.inputMode =
      config.keyboard === "number"
        ? "numeric"
        : config.keyboard === "phone"
          ? "tel"
          : config.keyboard === "email"
            ? "email"
            : "text";
    field.disabled = !!config.disabled;
    field._keyboard = config.keyboard || "text";
    field._secure = !!config.secure;
    el.classList.toggle("nl-disabled", !!config.disabled);
    field.dir = config.rtl ? "rtl" : "ltr";
  }

  op_iv([, id, text, count]) {
    const field = this.view(id)?._field;
    if (!field || count < field._count) return;
    field._known = text;
    if (field.value !== text) field.value = text;
  }

  op_ic([, id, x, y, w, h]) {
    const el = this.view(id);
    if (!el) return;
    el._content = [x, y, w, h];
    this.placeContent(el);
  }

  placeContent(el) {
    const field = el._field;
    if (!field || !el._content) return;
    const [x, y, w, h] = el._content;
    field.classList.remove("nl-inset");
    field.style.left = `${x}px`;
    field.style.top = `${y}px`;
    field.style.width = `${w}px`;
    field.style.height = `${h}px`;
  }

  op_cmd([, id, name, x, y, animated]) {
    const el = this.view(id);
    if (!el || name !== "scrollTo") return;
    if (!animated) this.noteInstant(el);
    el.scrollTo?.({ left: x, top: y, behavior: animated ? "smooth" : "instant" });
    if (!el.scrollTo) {
      el.scrollLeft = x;
      el.scrollTop = y;
    }
  }
}

/**
 * The pressable span under a click, or null. `_utf16` is a test stand-in
 * for the caret. A real click uses the document's caret at that point.
 * The innermost range wins.
 */
function linkAt(el, event) {
  const links = el._links;
  if (!links?.length) return null;
  const index = utf16At(el, event);
  if (index == null) return null;
  let node = null;
  let width = Infinity;
  for (const [start, end, id] of links) {
    if (index >= start && index < end && end - start < width) {
      node = id;
      width = end - start;
    }
  }
  return node;
}

function utf16At(el, event) {
  if (event._utf16 != null) return event._utf16;
  const doc = el.ownerDocument;
  const x = event.clientX;
  const y = event.clientY;
  if (typeof x !== "number" || typeof y !== "number" || !doc) return null;
  let node = null;
  let offset = 0;
  const pos = doc.caretPositionFromPoint?.(x, y);
  if (pos) {
    node = pos.offsetNode;
    offset = pos.offset;
  } else if (typeof doc.caretRangeFromPoint === "function") {
    const range = doc.caretRangeFromPoint(x, y);
    if (!range) return null;
    node = range.startContainer;
    offset = range.startOffset;
  } else {
    return null;
  }
  const box = el._text;
  if (!box || typeof box.contains !== "function" || !box.contains(node)) return null;
  if (node.nodeType === 3) {
    return utf16Before(box, node) + offset;
  }
  const child = node.childNodes?.[offset];
  return child ? utf16Before(box, child) : utf16Before(box, null);
}

function utf16Before(root, stop) {
  let count = 0;
  const walk = (node) => {
    if (node === stop) return true;
    if (node.nodeType === 3) {
      count += node.textContent.length;
      return false;
    }
    for (const child of node.childNodes ?? []) {
      if (walk(child)) return true;
    }
    return false;
  };
  walk(root);
  return count;
}
