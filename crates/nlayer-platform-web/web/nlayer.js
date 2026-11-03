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

import { GestureGate, attachGestures } from "./gesture.js";

/**
 * The style classes of the document. A class named after its CSS
 * (`nl-` and 8 hex digits, from the engine or `cssClass`) is one rule;
 * a view's own rule (`i<id>…`, `nl-p-` in front) is replaced as it changes.
 * Name → its CSS text.
 */
const CLASSES = { sheets: {}, rules: new Map(), texts: new Map() };

/** Roles whose ARIA name differs (`""`: no role attribute). */
const ARIA_ROLES = {
  imagebutton: "button",
  keyboardkey: "button",
  adjustable: "slider",
  header: "heading",
  text: "",
  summary: "",
};

/** Bits from `tailwind::state` the page reports. Focus is always sent. */
const PRESSED = 1;
const HOVERED = 2;
const FOCUSED = 4;

/** Pull-to-refresh thresholds (points). */
const REFRESH_PULL = 64;
const REFRESH_SPACE = 56;
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
 * `states(id, bits)`, `change(id, text)`, `scroll(id, name, x, y, contentWidth, contentHeight, width, height)`.
 * `name` is the event (`onScroll`, `onScrollEnd`, `onScrollDragStart`,
 * `onScrollDragEnd`, `onMomentumStart`, `onMomentumEnd`). Lengths are CSS
 * pixels. `assetBase` is prepended to an `asset:/` URI.
 */
export class Player {
  constructor(root, host = {}, options = {}) {
    this.root = root;
    this.host = host;
    this.animation = null;
    this.assetBase = options.assetBase ?? "";
    this.assetMap = options.assetMap ?? {};
    this.doc = root.ownerDocument ?? globalThis.document;
    this.views = new Map();
    this.pointers = new Map();
    this.pressHeld = new Map();
    this.focused = null;
    this.touchDrags = new Set();
    this.keyboard = 0;
    this.swallowTarget = null;
    this.onTouchEnd = (event) => this.touchEnded(event);
    root._id = 1;
    this.gestures = new GestureGate();
    attachGestures(this);
    this.wrapPointerState();
    root._kind = "view";
    root.classList.add("nl-v", "nl-root");
    this.views.set(1, root);
    root.addEventListener("pointerdown", (event) => this.pointerDown(event), true);
    root.addEventListener("pointercancel", (event) => this.pointerCancel(event), true);
    root.addEventListener("pointermove", (event) => this.pointerMove(event), true);
    root.addEventListener("pointerup", (event) => this.pointerUp(event), true);
    root.addEventListener("touchend", this.onTouchEnd);
    root.addEventListener("touchcancel", this.onTouchEnd);
    const view = this.doc.defaultView;
    if (view && view.addEventListener && view !== root) {
      view.addEventListener("touchend", this.onTouchEnd);
      view.addEventListener("touchcancel", this.onTouchEnd);
    }
    if (options.baseCSS != null) installBase(this.doc, options.baseCSS);
    this.historyDepth = 0;
    this.historySync = 0;
    this.ignoredPops = 0;
    this.window = this.doc.defaultView ?? globalThis;
    this.history = this.window.history;
    this.historySession = `${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`;
    this.onPopState = (e) => this.popState(e);
    this.onResize = () => this.windowResized();
    if (this.history && typeof this.history.pushState === "function") {
      this.history.replaceState(this.historyState(0), "");
      this.window.addEventListener?.("popstate", this.onPopState);
    }
    this.window.addEventListener?.("resize", this.onResize);
    this.onCopy = (event) => this.copyText(event);
    this.doc.addEventListener?.("copy", this.onCopy);
    this.safeArea = options.safeArea ?? null;
    this.changedStacks = new Set();
    this.changedModals = false;
  }

  /** `env(keyboard-inset-height)`. A pin of 0 leaves taps alone. */
  setKeyboard(height) {
    this.keyboard = height > 0 ? height : 0;
  }

  /** The page's animation graph: it writes bound values after the class. */
  setAnimation(animation) {
    this.animation = animation;
    for (const [id, el] of this.views) {
      if (el._bindings) animation?.bind(id, el, el._bindings);
    }
  }

  apply(batch) {
    for (const item of batch) {
      const handler = this[`op_${item[0]}`];
      if (handler) handler.call(this, item);
    }
    this.reconcileStacks();
  }

  view(id) {
    return this.views.get(id);
  }

  asset(uri) {
    if (!uri) return uri;
    if (uri.startsWith("asset:/")) {
      const path = uri.slice("asset:/".length).replace(/^\/+/, "");
      return this.assetBase + (this.assetMap[path] ?? path);
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
    el._disabled = false;
    if (kind === "scroll") {
      el.className = "nl-v nl-scroll";
    } else if (kind === "stack") {
      el.className = "nl-v nl-stack";
    } else if (kind === "screen") {
      el.className = "nl-v nl-screen";
    } else if (kind === "modal") {
      el.className = "nl-v nl-modal";
    } else if (kind === "headeritem") {
      el.className = "nl-v nl-headeritem";
    } else {
      el.className = "nl-v";
    }
    if (kind === "image") {
      const img = this.doc.createElement("img");
      img.draggable = false;
      img.className = "nl-fill nl-fit-cover";
      img.style.borderRadius = "inherit";
      img.style.overflow = "clip";
      el.append(img);
      el._img = img;
    }
    el.addEventListener("pointerdown", (event) => this.pressStart(el, event));
    el.addEventListener("pointerup", () => this.cancelLong(el));
    el.addEventListener("pointercancel", () => this.cancelLong(el));
    el.addEventListener("pointermove", (event) => {
      if (!el._pressDown) return;
      if (Math.hypot(event.clientX - el._pressX, event.clientY - el._pressY) > 10) {
        this.cancelLong(el);
      }
    });
    if (kind === "scroll") this.wireScroll(el);
    if (kind === "screen") {
      el.addEventListener("nlayer:insets", (e) => this.host.insets?.(id, e.detail));
    }
    if (kind === "modal") {
      el.addEventListener("nlayer:insets", (e) => this.host.insets?.(id, e.detail));
    }
    el.addEventListener("click", (event) => {
      for (let node = el; node; node = node.parentNode) {
        if (node._disabled) {
          event.stopPropagation?.();
          event.preventDefault?.();
          return;
        }
      }
      if (el._longPressed) {
        el._longPressed = false;
        event.stopPropagation?.();
        return;
      }
      if (this.swallowTarget) {
        const swallow = this.swallowTarget;
        this.swallowTarget = null;
        if (
          swallow === event.target ||
          swallow.contains?.(event.target) ||
          event.target?.contains?.(swallow)
        ) {
          event.stopPropagation?.();
          return;
        }
      }
      if (this.selecting(el)) {
        event.stopPropagation?.();
        return;
      }
      // The innermost `href` at the press goes to the engine with the
      // press: the core checks the address and mounts `OpenUrl`, and the
      // press still reports `onPress`.
      const target = hrefAt(el, event) ?? linkAt(el, event);
      if (target != null) {
        event.stopPropagation?.();
        this.host.press?.(target);
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
      if (name && el._kind !== "input") {
        el.tabIndex = 0;
        this.bindFocus(el, el);
        if (this.doc.activeElement === el) this.setState(el, FOCUSED, true);
      } else {
        el.tabIndex = undefined;
        el.removeAttribute?.("tabindex");
      }
    }
  }

  /**
   * `["ts", id, states]`. `states` is the mask to report (`PRESSED`,
   * `HOVERED`). Focus is reported either way. `0` drops press and hover.
   */
  op_ts([, id, states]) {
    const el = this.view(id);
    if (!el) return;
    el._tracked = states | 0;
    const kept = (el._states || 0) & (FOCUSED | (el._tracked & (PRESSED | HOVERED)));
    this.reportStates(el, kept);
    this.syncHover(el);
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
    this.trackChange(parent, child);
  }

  op_r([, id]) {
    const el = this.view(id);
    if (!el) return;
    const parent = el.parentNode;
    el.remove();
    if (parent) this.trackChange(parent, el);
  }

  op_d([, id]) {
    const el = this.view(id);
    if (!el) return;
    const parent = el.parentNode;
    if (this.focused === el) this.focused = null;
    for (const held of this.pressHeld.values()) held.delete(el);
    this.animation?.unbind(id);
    this.views.delete(id);
    el.remove();
    if (parent) this.trackChange(parent, el);
  }

  trackChange(parent, child) {
    if (child._kind === "screen" && parent?._kind === "stack") {
      this.changedStacks.add(parent);
      this.syncHistorySoon();
    } else if (child._kind === "modal") {
      this.changedModals = true;
      this.syncHistorySoon();
    }
  }

  reconcileStacks() {
    for (const stack of this.changedStacks) {
      if (!this.views.has(stack._id)) continue;
      const screens = [...stack.children].filter((c) => c._kind === "screen");
      screens.forEach((screen, i) => {
        const isTop = i === screens.length - 1;
        screen.classList.toggle("nl-top", isTop);
      });
    }
    this.changedStacks.clear();
    if (this.changedModals) {
      this.changedModals = false;
      for (const el of this.views.values()) {
        if (el._kind !== "modal" || !el.isConnected) continue;
        // A modal covers the window wherever the app rendered it.
        if (el.parentNode !== this.root) this.root.append(el);
        this.placeModal(el);
      }
    }
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

  op_t([
    ,
    id,
    x,
    y,
    w,
    h,
    runs,
    styles,
    spanIds,
    links,
    selectable,
    selection,
    hrefs,
    backgrounds,
    source,
  ]) {
    const el = this.view(id);
    if (!el) return;
    let box = el._text;
    if (!box) {
      box = this.doc.createElement("div");
      box.className = "nl-text";
      el._text = box;
      el.insertBefore(box, el.firstChild);
      // A selection the browser starts is not a long press, and the click
      // that ends it reports nothing (see `selecting`).
      box.addEventListener("selectstart", () => this.selectStart(el));
    }
    const s = box.style;
    s.left = `${x}px`;
    s.top = `${y}px`;
    s.width = `${w}px`;
    s.height = `${h}px`;
    // Runs are where layout put them, inside the content box. The browser
    // does not wrap them: an attachment's frame uses the same origin.
    this.placeRuns(box, runs || [], styles || [], backgrounds || []);
    el._spanIds = spanIds || [];
    el._links = links || [];
    el._hrefs = hrefs || [];
    el._source = typeof source === "string" ? source : null;
    // `selectable` is tri-state: `true` is `userSelect: "text"`, `false`
    // `"none"`, `null` follows the views around it (and is not selectable).
    el.classList.toggle("nl-selectable", selectable === true);
    el.classList.toggle("nl-noselect", selectable === false);
    el._selectable = selectable === true;
    el._noselect = selectable === false;
    if (selection) el.style.setProperty?.("--nl-selection", selection);
    else el.style.removeProperty?.("--nl-selection");
    el._selection = selection || null;
  }

  /** Glyph runs and span backgrounds, in the content box. */
  placeRuns(box, runs, styles, backgrounds) {
    const nodes = [];
    for (const [x, y, w, h, r, color] of backgrounds) {
      const bg = this.doc.createElement("div");
      bg.className = "nl-run-bg";
      bg.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px;border-radius:${r}px;background:${color}`;
      nodes.push(bg);
    }
    for (const [x, y, text, style, start, after] of runs) {
      const span = this.doc.createElement("span");
      const name = this.styleClass(styles[style]);
      span.className = name ? `nl-run ${name}` : "nl-run";
      span.style.left = `${x}px`;
      span.style.top = `${y}px`;
      span.textContent = text ?? "";
      if (start != null) span._start = start;
      if (after) span._after = after;
      nodes.push(span);
    }
    // The host stand-in clears and ignores arguments. A real node does too
    // when called with none, then `append` places the runs.
    box.replaceChildren();
    for (const node of nodes) box.append(node);
  }

  /** `selectable` on a view: its texts select as one area. */
  op_sel([, id, selectable]) {
    const el = this.view(id);
    if (!el) return;
    el.classList.toggle("nl-area", !!selectable);
    el._area = !!selectable;
  }

  /**
   * The browser began selecting inside text `el`: the press in progress is
   * a selection, not a long press (and a click after it reports nothing).
   */
  selectStart(el) {
    el._selected = true;
    for (let node = el; node; node = node.parentNode) this.cancelLong(node);
  }

  /** Whether text `el` is selectable (`userSelect: "text"`, or a `selectable` view around it). */
  selectableText(el) {
    for (let node = el; node; node = node.parentNode) {
      if (node._noselect) return false;
      if (node._selectable || node._area) return true;
    }
    return false;
  }

  /** The click that ends a selection, if this one does. */
  selecting(el) {
    if (el._selected) {
      el._selected = false;
      return true;
    }
    const selection = this.window?.getSelection?.();
    if (!selection || selection.isCollapsed || !selection.rangeCount) return false;
    const anchor = selection.anchorNode;
    if (!anchor) return false;
    const scope = this.selectArea(el) ?? el;
    const node = scope._text ?? scope;
    return node === anchor || node.contains?.(anchor) === true;
  }

  /** The outermost `selectable` view around `el`, if any. */
  selectArea(el) {
    let area = null;
    for (let node = el; node; node = node.parentNode) if (node._area) area = node;
    return area;
  }

  op_lp([, id, delay]) {
    const el = this.view(id);
    if (el) el._longPressDelay = delay == null ? 500 : delay;
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

  /**
   * Accessibility: `[a, id, role, label, hint, hidden, disabled, selected,
   * checked, expanded, min, max, now, valueText]`. `checked` is 0 none, 1
   * false, 2 true, 3 mixed; `expanded` 0 none, 1 false, 2 true.
   */
  op_a(item) {
    const [
      ,
      id,
      role,
      label,
      hint,
      hidden,
      disabled,
      selected,
      checked,
      expanded,
      min,
      max,
      now,
      valueText,
    ] = item;
    const el = this.view(id);
    if (!el) return;
    el._disabled = !!disabled;
    el.classList.toggle("nl-disabled", !!disabled);
    if (disabled) this.setState(el, PRESSED, false);
    const target = el._field ?? el;
    const set = (name, value) =>
      value == null || value === false
        ? target.removeAttribute(name)
        : target.setAttribute(name, String(value));
    const aria = role === "none" ? null : (ARIA_ROLES[role] ?? role ?? null);
    set("role", aria === "" ? null : aria);
    set("aria-label", label);
    set("aria-description", hint);
    set("aria-hidden", hidden ? "true" : null);
    set("aria-disabled", disabled ? "true" : null);
    set("aria-selected", selected ? "true" : null);
    set(
      "aria-checked",
      checked === 3 ? "mixed" : checked === 2 ? "true" : checked === 1 ? "false" : null,
    );
    set("aria-expanded", expanded === 2 ? "true" : expanded === 1 ? "false" : null);
    set("aria-valuemin", min);
    set("aria-valuemax", max);
    set("aria-valuenow", now);
    set("aria-valuetext", valueText);
    el._a11y = {
      role: role || null,
      label: label ?? null,
      hint: hint ?? null,
      hidden: !!hidden,
      state: { disabled: !!disabled, selected: !!selected, checked, expanded },
      value: { min, max, now, text: valueText ?? null },
    };
  }

  /** Animated properties: `[id, [[property, node], …]]`. */
  op_ab([, id, bindings]) {
    const el = this.view(id);
    if (!el) return;
    el._bindings = bindings;
    this.animation?.bind(id, el, bindings);
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

  op_xf([, id, steps]) {
    const el = this.view(id);
    if (!el) return;
    el._transform = Array.isArray(steps) && steps.length ? steps : null;
    if (el._bindings) this.animation?.bind(id, el, el._bindings);
  }

  op_pb([, id, x, y]) {
    const el = this.view(id);
    if (!el) return;
    el._panNodes = x != null || y != null ? [x, y] : null;
  }

  op_pinchb([, id, scale]) {
    const el = this.view(id);
    if (!el) return;
    el._pinchNode = scale != null ? scale : null;
  }

  op_rotb([, id, angle]) {
    const el = this.view(id);
    if (!el) return;
    el._rotationNode = angle != null ? angle : null;
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
    el._pull = 0;
    el.addEventListener("scroll", () => this.scrolled(el), { passive: true });
    el.addEventListener("scrollend", () => this.scrollEnded(el));
    // The user takes over an instant command (`noteInstant`).
    const takeOver = () => {
      el._swallowEnd = false;
    };
    for (const type of ["wheel", "pointerdown", "touchstart", "keydown"]) {
      el.addEventListener(type, takeOver, { passive: true });
    }

    // Pull-to-refresh touch handling
    let startY = 0;
    let pulling = false;
    el.addEventListener(
      "touchstart",
      (e) => {
        if (e.touches.length === 1 && this.canPull(el)) {
          startY = e.touches[0].clientY;
          pulling = false;
        }
      },
      { passive: true },
    );

    el.addEventListener(
      "touchmove",
      (e) => {
        if (e.touches.length !== 1 || !this.canPull(el)) return;
        const dy = e.touches[0].clientY - startY;
        const scrollTop = el.scrollTop - (el._pull || 0);
        if (scrollTop <= 0 && dy > 0) {
          if (!e.cancelable) {
            pulling = false;
            return;
          }
          e.preventDefault();
          pulling = true;
          this.dragPull(el, -dy);
        }
      },
      { passive: false },
    );

    const endPull = () => {
      if (pulling) {
        pulling = false;
        this.releasePull(el);
      }
    };
    el.addEventListener("touchend", endPull);
    el.addEventListener("touchcancel", endPull);
  }

  canPull(el) {
    return !el._horizontal && el._events?.has("onRefresh") && el.scrollTop - (el._pull || 0) <= 0.5;
  }

  dragPull(el, distance) {
    // Apply rubber band resistance: (1 - 1/(over*0.55/dim + 1)) * dim
    const viewport = el.clientHeight;
    const resistedPull =
      viewport > 0 ? (1 - 1 / ((distance * 0.55) / viewport + 1)) * viewport : distance * 0.5;
    this.setPull(el, resistedPull);
  }

  releasePull(el) {
    const threshold = el._behavior?.refreshIndicator?.height ?? 64;
    if (!el._refreshing && el._pull >= threshold) {
      el._refreshing = true;
      this.scrollEvent(el, "onRefresh");
    }
    const target = el._refreshing ? (el._behavior?.refreshIndicator?.height ?? 56) : 0;
    this.animatePull(el, target);
  }

  setPull(el, pull) {
    if (pull === el._pull) return;
    el._pull = pull;
    if (el._spacer) {
      el._spacer.style.paddingTop = `${pull}px`;
    }
    // Report pull distance (negative offset means pull)
    this.host.scroll?.(
      el._id,
      "onScroll",
      0,
      -pull,
      el._contentW || 0,
      el._contentH || 0,
      el.clientWidth,
      el.clientHeight,
    );
  }

  animatePull(el, target) {
    cancelAnimationFrame(el._pullAnimation);
    const from = el._pull || 0;
    if (from === target) return;
    const t0 = this.now();
    const step = (now) => {
      const t = Math.min(1, (now - t0) / 250);
      const eased = 1 - Math.pow(1 - t, 3);
      this.setPull(el, from + (target - from) * eased);
      if (t < 1) el._pullAnimation = requestAnimationFrame(step);
    };
    el._pullAnimation = requestAnimationFrame(step);
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

  beginDrag(el) {
    if (!el || el._touchDrag) return;
    if (this.keyboard > 0 && el._dismiss && el._dismiss !== "none") this.blurField();
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
    // Fire onScrollEnd immediately when drag ends
    // The armRest timer is still needed for non-touch scrolling
    this.scrollEvent(el, "onScrollEnd");
  }

  pressStart(el, event) {
    this.cancelLong(el);
    el._longPressed = false;
    el._selected = false;
    if (event.button != null && event.button !== 0) return;
    this.markPressed(el, this.hold(event.pointerId ?? 0));
    if (!el._events.has("onLongPress")) return;
    // On selectable text a long press starts a selection: no timer, and so
    // no `onLongPress`.
    if (el._kind === "text" && this.selectableText(el)) return;
    for (let node = el; node; node = node.parentNode) {
      if (node._disabled) return;
    }
    el._pressDown = true;
    el._pressX = event.clientX;
    el._pressY = event.clientY;
    el._longTimer = setTimeout(() => {
      el._longTimer = 0;
      if (!el._pressDown || this.views.get(el._id) !== el) return;
      el._longPressed = true;
      this.host.longPress?.(el._id);
    }, el._longPressDelay ?? 500);
  }

  cancelLong(el) {
    el._pressDown = false;
    if (el._longTimer) {
      clearTimeout(el._longTimer);
      el._longTimer = 0;
    }
  }

  /** Root `pointerDown` / `pointerUp` / `pointerCancel` also carry press. */
  wrapPointerState() {
    const down = this.pointerDown;
    const up = this.pointerUp;
    const cancel = this.pointerCancel;
    this.pointerDown = (event) => {
      this.pressDown(event);
      down.call(this, event);
    };
    this.pointerUp = (event) => {
      this.pressUp(event);
      up.call(this, event);
    };
    this.pointerCancel = (event) => {
      this.pressUp(event);
      cancel.call(this, event);
    };
  }

  pressDown(event) {
    if (event.button != null && event.button !== 0) return;
    const held = this.hold(event.pointerId ?? 0);
    for (let node = event.target; node; node = node.parentNode) this.markPressed(node, held);
  }

  pressUp(event) {
    const held = this.pressHeld.get(event.pointerId ?? 0);
    if (!held) return;
    this.pressHeld.delete(event.pointerId ?? 0);
    for (const el of held) {
      let still = false;
      for (const other of this.pressHeld.values()) {
        if (other.has(el)) {
          still = true;
          break;
        }
      }
      if (!still) this.setState(el, PRESSED, false);
    }
  }

  hold(pointerId) {
    let held = this.pressHeld.get(pointerId);
    if (!held) {
      held = new Set();
      this.pressHeld.set(pointerId, held);
    }
    return held;
  }

  /** Bit 1 while the pointer is down on `el`, and only when that bit is tracked. */
  markPressed(el, held) {
    if (!el || el._id == null || this.pressBlocked(el)) return;
    if (((el._tracked || 0) & PRESSED) === 0) return;
    this.setState(el, PRESSED, true);
    held.add(el);
  }

  /** The view, or its field, is disabled. Ancestors the host already skips count too. */
  pressBlocked(el) {
    for (let node = el; node; node = node.parentNode) {
      if (node._disabled || node._field?.disabled) return true;
    }
    return false;
  }

  syncHover(el) {
    const want = ((el._tracked || 0) & HOVERED) !== 0;
    if (want && !el._hoverListeners) {
      el._onPointerEnter = (event) => {
        if (event.pointerType && event.pointerType !== "mouse") return;
        this.setState(el, HOVERED, true);
      };
      el._onPointerLeave = () => this.setState(el, HOVERED, false);
      el.addEventListener("pointerenter", el._onPointerEnter);
      el.addEventListener("pointerleave", el._onPointerLeave);
      el._hoverListeners = true;
    } else if (!want && el._hoverListeners) {
      el.removeEventListener("pointerenter", el._onPointerEnter);
      el.removeEventListener("pointerleave", el._onPointerLeave);
      el._onPointerEnter = null;
      el._onPointerLeave = null;
      el._hoverListeners = false;
      this.setState(el, HOVERED, false);
    }
  }

  /** Focus and blur on `target` report bit 4 for view `el` only. */
  bindFocus(target, el) {
    if (!target || target._focusBound) return;
    target._focusBound = true;
    target.addEventListener("focus", () => this.setState(el, FOCUSED, true));
    target.addEventListener("blur", () => this.setState(el, FOCUSED, false));
  }

  reportStates(el, next) {
    if ((el._states || 0) === next) return;
    el._states = next;
    this.host.states?.(el._id, next);
  }

  setState(el, bit, on) {
    if (!el || el._id == null) return;
    if (bit === PRESSED && on && this.pressBlocked(el)) return;
    if ((((el._tracked || 0) | FOCUSED) & bit) === 0) return;
    const states = el._states || 0;
    const next = on ? states | bit : states & ~bit;
    if (next === states) return;
    el._states = next;
    if (bit === FOCUSED) {
      if (on) {
        const prev = this.focused;
        this.focused = el;
        if (prev && prev !== el) this.setState(prev, FOCUSED, false);
      } else if (this.focused === el) this.focused = null;
    }
    this.host.states?.(el._id, el._states);
  }

  scrolled(el) {
    el._settled = false;
    // Still moving after the finger left: a glide.
    if (el._released && !el._momentum && this.now() - el._released < 150) {
      el._momentum = true;
      el._released = 0;
      this.scrollEvent(el, "onMomentumStart");
    }
    // Lists follow the offset even without an `onScroll` listener.
    this.queueScroll(el);
    if (!el._touchDrag && !this.holding && !el._swallowEnd) this.armRest(el);
  }

  /** The latest offset, once per frame when the page has a frame clock. */
  queueScroll(el) {
    if (el._scrollQueued) return;
    el._scrollQueued = true;
    const send = () => {
      el._scrollQueued = false;
      if (this.views.get(el._id) !== el) return;
      this.reportScroll(el, "onScroll");
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

    // Check for snap target before settling
    if (el._snapConfig && this.host.snap) {
      const horizontal = el._horizontal ?? false;
      const offset = horizontal ? el.scrollLeft : el.scrollTop;
      const viewport = horizontal ? el.clientWidth : el.clientHeight;
      const content = horizontal ? el.scrollWidth : el.scrollHeight;
      const velocity = 0; // scrollend means already stopped

      const cfg = el._snapConfig;
      const target = this.host.snap(
        offset,
        velocity,
        viewport,
        content,
        cfg.paging,
        cfg.snapInterval,
        cfg.snapOffsets,
        cfg.snapAlignment,
        cfg.disableIntervalMomentum,
        cfg.deceleration,
      );

      if (target >= 0 && Math.abs(target - offset) > 0.5) {
        // Snap to target
        this.noteInstant(el);
        el.scrollTo({
          [horizontal ? "left" : "top"]: target,
          behavior: "smooth",
        });
        return; // Don't fire onScrollEnd yet, wait for snap to complete
      }
    }

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
    this.reportScroll(el, name);
  }

  /** Sends the offset whatever is listening: the core needs it for lists. */
  reportScroll(el, name) {
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

  op_sc([, id, horizontal, taps, dismiss]) {
    const el = this.view(id);
    if (!el) return;
    el._horizontal = horizontal;
    el._taps = taps || "never";
    el._dismiss = dismiss || "none";
    swapClass(el, "_overflow", horizontal ? "nl-ox" : "nl-oy");
  }

  op_scs([
    ,
    id,
    w,
    h,
    refreshIndicator,
    paging,
    snapInterval,
    snapOffsets,
    snapAlignment,
    disableIntervalMomentum,
    deceleration,
  ]) {
    const el = this.view(id);
    if (!el?._spacer) return;
    el._spacer.style.width = `${w}px`;
    el._spacer.style.height = `${h}px`;
    // Store snap configuration
    el._snapConfig = {
      paging: paging ?? false,
      snapInterval: snapInterval ?? 0,
      snapOffsets: snapOffsets ?? null,
      snapAlignment: snapAlignment ?? 0,
      disableIntervalMomentum: disableIntervalMomentum ?? false,
      deceleration: deceleration ?? 0,
    };
  }

  op_ref([, id, refreshing]) {
    const el = this.views.get(id);
    if (!el) return;
    el._refreshing = refreshing;
    if (refreshing) {
      // Ensure pull indicator shows
      if (el._pull < (el._behavior?.refreshIndicator?.height ?? 56)) {
        this.animatePull(el, el._behavior?.refreshIndicator?.height ?? 56);
      }
    } else {
      // Hide pull indicator
      this.animatePull(el, 0);
    }
  }

  op_lc([, id, config]) {
    const el = this.views.get(id);
    if (!el) return;
    el._listConfig = config;
    // A list in its parent's viewport is a plain clipped view as long as
    // its content; the scroll view around it does the scrolling.
    if (config.viewportParent) el.style.overflow = "hidden";
    // Core handles sticky/inverted via frame calculations
    // Platform just applies the frames
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
    const tag = config.multiline ? "TEXTAREA" : "INPUT";
    let field = el._field;
    if (field && field.tagName !== tag) {
      const old = field;
      const focused = this.doc.activeElement === old;
      field = this.makeField(el, id, tag);
      field.value = old.value;
      field._known = old._known ?? old.value;
      field._count = old._count ?? 0;
      old.parentNode?.insertBefore(field, old);
      old.remove();
      if (focused) field.focus();
    } else if (!field) {
      field = this.makeField(el, id, tag);
      el.append(field);
      if (el._content) this.placeContent(el);
    }
    swapClass(field, "_class", this.styleClass(css));
    field.placeholder = config.placeholder;
    if (config.multiline) {
      field.removeAttribute("type");
      field.removeAttribute("inputMode");
    } else {
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
    }
    field.disabled = !!config.disabled;
    field._keyboard = config.keyboard || "text";
    field._secure = !!config.secure;
    field._multiline = !!config.multiline;
    el.classList.toggle("nl-disabled", !!config.disabled);
    if (config.disabled) this.setState(el, PRESSED, false);
    field.dir = config.rtl ? "rtl" : "ltr";
  }

  /** A field for `el`: a textarea when the input takes several lines. */
  makeField(el, id, tag) {
    const field = this.doc.createElement(tag);
    field.className = "nl-field nl-inset";
    field._count = 0;
    field._known = "";
    field._selection = [0, 0];
    field.addEventListener("input", () => this.textChanged(el, id, field));
    field.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") return;
      if (field.tagName === "TEXTAREA") {
        // Return inserts a line break (`submitBehavior` is not part of v2).
        // An input method's Return commits its composition instead.
        if (event.isComposing || event.keyCode === 229) return;
        event.preventDefault();
        const start = field.selectionStart ?? field.value.length;
        const end = field.selectionEnd ?? start;
        field.setRangeText("\n", start, end, "end");
        this.textChanged(el, id, field);
        this.selectionChanged(el, id, field);
        return;
      }
      const kind = field._keyboard;
      if (kind && kind !== "text" && kind !== "email") return;
      event.preventDefault();
      field.blur();
    });
    // A composition is held from JS while it lasts: setting a field's
    // value ends it and commits the pre-edit text.
    field.addEventListener("compositionstart", (event) => {
      field._composing = true;
      this.compositionChanged(el, id, field, event.data ?? "");
    });
    field.addEventListener("compositionupdate", (event) =>
      this.compositionChanged(el, id, field, event.data ?? ""),
    );
    field.addEventListener("compositionend", () => {
      field._composing = false;
      // The committed text is reported before the end (browsers differ in
      // whether its `input` event comes before or after).
      this.textChanged(el, id, field);
      this.compositionChanged(el, id, field, "");
      const pending = field._pending;
      field._pending = null;
      if (pending) this.showValue(field, pending[0], pending[1]);
    });
    field.addEventListener("select", () => this.selectionChanged(el, id, field));
    field.addEventListener("keyup", () => this.selectionChanged(el, id, field));
    field.addEventListener("pointerup", () => this.selectionChanged(el, id, field));
    this.bindFocus(field, el);
    el._field = field;
    return field;
  }

  textChanged(el, id, field) {
    if (field.disabled) return;
    if (field.value === field._known) return;
    field._known = field.value;
    field._count++;
    this.host.change?.(id, field.value);
  }

  compositionChanged(el, id, field, text) {
    const composing = !!field._composing;
    const last = field._composition;
    if (last && last[0] === composing && last[1] === text) return;
    field._composition = [composing, text];
    this.host.composition?.(id, composing, text);
  }

  selectionChanged(el, id, field) {
    const selection = [field.selectionStart ?? 0, field.selectionEnd ?? 0];
    const last = el._selection;
    if (last && last[0] === selection[0] && last[1] === selection[1]) return;
    el._selection = selection;
    this.host.selection?.(id, selection[0], selection[1]);
  }

  op_iv([, id, text, count]) {
    const field = this.view(id)?._field;
    if (!field) return;
    if (field._composing) {
      field._pending = [text, count];
      return;
    }
    field._pending = null;
    this.showValue(field, text, count);
  }

  showValue(field, text, count) {
    if (count < field._count) return;
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
    if (name === "blur") {
      this.blurField();
      return;
    }
    // A `Text` `href` the core checked: one way, no report back (a blocked
    // popup is not an error the app sees).
    if (name === "openUrl") {
      this.window?.open?.(x, "_blank");
      return;
    }
    const el = this.view(id);
    if (!el) return;
    if (name === "setSelection") {
      const field = el._field;
      if (field) {
        const length = field.value.length;
        const start = Math.min(x, length),
          end = Math.min(Math.max(y, x), length);
        el._selection = [start, end];
        field.setSelectionRange(start, end);
      }
      return;
    }
    if (name === "scrollToEnd") {
      if (!animated) this.noteInstant(el);
      const [x, y] = el._horizontal ? [el.scrollWidth, 0] : [0, el.scrollHeight];
      el.scrollTo?.({
        left: x,
        top: y,
        behavior: animated ? "smooth" : "instant",
      });
      if (!el.scrollTo) {
        el.scrollLeft = x;
        el.scrollTop = y;
      }
      return;
    }
    if (name !== "scrollTo") return;
    if (!animated) this.noteInstant(el);

    // For virtual lists, use instant scroll for large distances to avoid interruption
    // Smooth scroll can be interrupted by content changes during scroll
    const distance = Math.abs((el.scrollTop || 0) - y) + Math.abs((el.scrollLeft || 0) - x);
    const useInstant = !animated || distance > 5000;

    el.scrollTo?.({
      left: x,
      top: y,
      behavior: useInstant ? "instant" : "smooth",
    });
    if (!el.scrollTo) {
      el.scrollLeft = x;
      el.scrollTop = y;
    }
  }

  op_scr([, id, config]) {
    const el = this.view(id);
    if (!el) return;
    el._screenConfig = config;
    this.syncHistorySoon();
  }

  op_mc([, id, config]) {
    const el = this.view(id);
    if (!el) return;
    el._modalConfig = config;
    this.placeModal(el);
    this.syncHistorySoon();
  }

  syncHistorySoon() {
    if (this.historySync) return;
    this.historySync = setTimeout(() => {
      this.historySync = 0;
      this.syncHistory();
    }, 0);
  }

  placeModal(el) {
    if (!el || el._kind !== "modal") return;
    const [width, height] = this.rootSize();
    const [top, right, bottom, left] = this.readInsets();
    const last = el._lastPlace;
    if (
      last &&
      last[0] === width &&
      last[1] === height &&
      last[2] === top &&
      last[3] === right &&
      last[4] === bottom &&
      last[5] === left
    )
      return;
    el._lastPlace = [width, height, top, right, bottom, left];
    this.host.setRootSize?.(el._id, width, height, top, right, bottom, left);
  }

  rootSize() {
    return [this.root.clientWidth || 0, this.root.clientHeight || 0];
  }

  readInsets() {
    if (this.safeArea) {
      const area = this.safeArea;
      return [area.top || 0, area.right || 0, area.bottom || 0, area.left || 0];
    }
    if (typeof getComputedStyle !== "function") {
      return [0, 0, 0, 0];
    }
    const probe = this.doc.createElement("div");
    probe.style.cssText =
      "position:absolute;visibility:hidden;padding:" +
      "env(safe-area-inset-top) env(safe-area-inset-right) " +
      "env(safe-area-inset-bottom) env(safe-area-inset-left)";
    this.root.append(probe);
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

  windowResized() {
    for (const el of this.views.values()) {
      if (el._kind === "modal" && el.isConnected) this.placeModal(el);
    }
  }

  historyState(depth) {
    return { ...(this.history.state ?? {}), nlayer: depth, nlayerSession: this.historySession };
  }

  syncHistory() {
    const depth = this.backDepth();
    const have = this.historyDepth || 0;
    for (let i = have + 1; i <= depth; i++) {
      this.history.pushState(this.historyState(i), "");
    }
    if (depth < have) {
      this.ignoredPops++;
      this.history.go(depth - have);
    }
    this.historyDepth = depth;
  }

  backDepth() {
    let depth = 0;
    for (const el of this.root.children) {
      if (el._kind === "modal") depth++;
    }
    for (const el of this.views.values()) {
      if (el._kind === "stack" && el.isConnected) {
        const screens = [...el.children].filter((c) => c._kind === "screen");
        depth += Math.max(0, screens.length - 1);
      }
    }
    return depth;
  }

  popState(e) {
    if (this.ignoredPops > 0) {
      this.ignoredPops--;
      return;
    }
    const have = this.historyDepth;
    if (e.state?.nlayerSession !== this.historySession) {
      this.historyDepth = 0;
      // Blind loop: host.back() posts to the engine and returns immediately,
      // not a boolean result. Each call queues one back event.
      for (let i = 0; i < have; i++) this.host.back?.();
      if (e.state?.nlayer != null) this.history.back();
      return;
    }
    const at = e.state.nlayer ?? 0;
    if (at < have) {
      this.historyDepth = at;
      // Blind loop: host.back() posts to the engine and returns immediately,
      // not a boolean result. Each call queues one back event.
      for (let i = at; i < have; i++) this.host.back?.();
      this.syncHistorySoon();
    } else if (at > have) {
      this.ignoredPops++;
      this.history.go(have - at);
    }
  }

  blurField() {
    const active = this.doc.activeElement;
    if (active && active.classList?.contains("nl-field")) active.blur();
  }

  nearestScroll(node) {
    for (let el = node; el; el = el.parentNode) {
      if (el._kind === "scroll") return el;
    }
    return null;
  }

  onInput(node, scroll) {
    for (let el = node; el && el !== scroll; el = el.parentNode) {
      if (el._kind === "input" || el.classList?.contains("nl-field")) return true;
    }
    return false;
  }

  handles(node, scroll, event) {
    for (let el = node; el; el = el.parentNode) {
      if (el._kind === "input" || el.classList?.contains("nl-field")) return true;
      if (el._events?.has("onPress")) return true;
      if (hrefAt(el, event) != null) return true;
      if (linkAt(el, event) != null) return true;
      if (el === scroll) return false;
    }
    return false;
  }

  noteKeyboardTap(event) {
    if (event.button > 0 || !(this.keyboard > 0)) return;
    const scroll = this.nearestScroll(event.target);
    if (!scroll) return;
    const onInput = this.onInput(event.target, scroll);
    const handled = this.handles(event.target, scroll, event);
    const taps = scroll._taps || "never";
    const dismiss =
      taps === "always" ? false : taps === "handled" ? !handled && !onInput : !onInput;
    if (!dismiss) return;
    this.blurField();
    if (taps === "never" && !onInput) {
      this.swallowTarget = event.target;
      event.preventDefault?.();
      event.stopPropagation?.();
    }
  }

  /**
   * Copy reads the source, placeholders included. The runs omit the
   * U+FFFC, so the DOM selection would drop them.
   */
  copyText(event) {
    const sel = this.doc.getSelection?.();
    const selected = String(sel ?? "");
    if (!selected) return;
    let node = sel?.anchorNode ?? null;
    while (node && !node.classList?.contains?.("nl-text")) node = node.parentNode;
    const source = node?.parentElement?._source;
    if (typeof source !== "string" || !source.includes("\uFFFC")) return;
    const flat = source.replaceAll("\uFFFC", "");
    const at = flat.indexOf(selected);
    if (at < 0) return;
    const start = sourceIndex(source, at);
    const end = sourceIndex(source, at + selected.length);
    event.clipboardData?.setData?.("text/plain", source.slice(start, end));
    event.preventDefault?.();
  }

  dispose() {
    if (this.onPopState) {
      this.window.removeEventListener?.("popstate", this.onPopState);
      this.onPopState = null;
    }
    if (this.onResize) {
      this.window.removeEventListener?.("resize", this.onResize);
      this.onResize = null;
    }
    if (this.onCopy) {
      this.doc.removeEventListener?.("copy", this.onCopy);
      this.onCopy = null;
    }
  }
}

/**
 * The node of the innermost `href` under a click, or null. Same caret as
 * [`linkAt`]; a span's range wins over its block's.
 */
function hrefAt(el, event) {
  const hrefs = el._hrefs;
  if (!hrefs?.length) return null;
  const index = utf16At(el, event);
  if (index == null) return null;
  let node = null;
  let width = Infinity;
  for (const [start, end, id] of hrefs) {
    if (index >= start && index < end && end - start < width) {
      node = id;
      width = end - start;
    }
  }
  return node;
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
  // A hit on the run element itself is its first source offset.
  if (node._start != null && node.nodeType !== 3) return node._start;
  return utf16Before(box, node) + (node.nodeType === 3 ? offset : 0);
}

function utf16Before(root, stop) {
  let count = 0;
  const walk = (node) => {
    if (node === stop) return true;
    if (node.nodeType === 3) {
      count += node.textContent.length;
      return false;
    }
    // Runs are absolutely positioned, so DOM order is not a fallback, and
    // a placeholder is not a character of the span. The run's start is.
    if (node._start != null && node.contains?.(stop)) count = node._start;
    for (const child of node.childNodes ?? []) {
      if (walk(child)) return true;
    }
    if (node._after) count += node._after.length;
    return false;
  };
  walk(root);
  return count;
}

/** A flat offset (U+FFFC removed) as an offset in `source`. */
function sourceIndex(source, flat) {
  let seen = 0;
  for (let i = 0; i < source.length; i++) {
    if (seen === flat) return i;
    if (source[i] !== "\uFFFC") seen++;
  }
  return source.length;
}
