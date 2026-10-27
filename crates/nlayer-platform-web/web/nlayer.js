// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
// The page's side of a mount batch (`Encoder` in this crate). Extracted
// from nsh's DOM mounter and cut to views, text, images, one-axis scroll
// views and single-line inputs: absolute frames, generated classes, a
// border overlay, a scroll spacer and a field whose value ignores a stale
// edit count.

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
 * `change(id, text)`, `scroll(id, x, y)`. `assetBase` is prepended to an
 * `asset:/` URI.
 */
export class Player {
  constructor(root, host = {}, options = {}) {
    this.root = root;
    this.host = host;
    this.assetBase = options.assetBase ?? "";
    this.doc = root.ownerDocument ?? globalThis.document;
    this.views = new Map();
    root._id = 1;
    root._kind = "view";
    root.classList.add("nl-v", "nl-root");
    this.views.set(1, root);
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
    if (kind === "scroll") {
      el._spacer = this.doc.createElement("div");
      el._spacer.className = "nl-spacer";
      el.append(el._spacer);
      el._horizontal = false;
      el.addEventListener("scroll", () => {
        if (el._events.has("onScroll")) this.host.scroll?.(id, el.scrollLeft, el.scrollTop);
      });
    }
    el.addEventListener("click", (event) => {
      if (!el._events.has("onPress")) return;
      event.stopPropagation?.();
      this.host.press?.(id);
    });
    this.views.set(id, el);
  }

  op_i([, parentId, childId, beforeId]) {
    const parent = this.view(parentId);
    const child = this.view(childId);
    if (!parent || !child) return;
    const before = beforeId == null ? null : this.view(beforeId);
    parent.insertBefore(child, before && before.parentNode === parent ? before : parent._spacer ?? null);
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

  op_t([, id, x, y, w, h, align, rtl, runs]) {
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
  }

  op_e([, id, events]) {
    const el = this.view(id);
    if (!el) return;
    el._events = new Set(events);
  }

  op_img([, id, uri, mode]) {
    const img = this.view(id)?._img;
    if (!img) return;
    img.className = `nl-fill nl-fit-${mode || "cover"}`;
    const src = this.asset(uri ?? "");
    if (!src) img.removeAttribute("src");
    else img.src = src;
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
    field.type = config.secure ? "password" : config.keyboard === "email" ? "email" : config.keyboard === "phone" ? "tel" : "text";
    field.inputMode = config.keyboard === "number" ? "numeric" : config.keyboard === "phone" ? "tel" : config.keyboard === "email" ? "email" : "text";
    field.disabled = !!config.disabled;
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
    el.scrollTo?.({ left: x, top: y, behavior: animated ? "smooth" : "instant" });
    if (!el.scrollTo) {
      el.scrollLeft = x;
      el.scrollTop = y;
    }
  }
}
