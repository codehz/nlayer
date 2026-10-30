// `<List>` for the page: the core chooses the visible items and recycles
// slots; this facade renders each slot's content and follows data changes.
// The QuickJS host's `list.rs` has the same shape.

const own = new Set([
  "each",
  "children",
  "ref",
  "header",
  "footer",
  "empty",
  "separator",
  "keyExtractor",
  "estimatedItemSize",
  "numColumns",
  "initialScrollIndex",
  "loadMoreThreshold",
]);

/**
 * @param {{
 *   invoke: (op: number, args?: any) => any,
 *   values: any,
 *   reactive: any,
 *   rt: any,
 *   lists: Map<number, any>,
 *   wake: () => void,
 * }} deps
 */
export function createLists({ invoke, values, reactive, rt, lists, wake }) {
  const root = (fn) => {
    let dispose;
    const value = invoke(105, (d) => {
      dispose = d;
      return fn(d);
    });
    return { value, dispose };
  };
  const slot = (value) =>
    typeof value === "function" && values.nodeIds.get(value) === undefined
      ? reactive.untracked(() => value({}))
      : value;

  function render(props = {}) {
    const node = rt.element("list");
    const id = values.nodeIds.get(node);
    const state = {
      node,
      props,
      cells: new Map(),
      extras: [],
      previous: undefined,
      forwarded: new Map(),
    };
    lists.set(id, state);
    reactive.cleanup(() => {
      lists.delete(id);
      for (const entry of [...state.cells.values(), ...state.extras]) {
        try {
          entry.dispose();
        } catch {}
      }
      state.cells.clear();
      state.extras.length = 0;
    });
    // The items and how they changed. The core keeps measurements with the
    // items that stayed.
    reactive.renderEffect(() => {
      const next = props.each ?? [];
      const old = state.previous ?? [];
      if (old.length && old !== next) {
        let start = 0;
        let end = old.length;
        let nextEnd = next.length;
        while (start < end && start < nextEnd && old[start] === next[start]) start++;
        while (end > start && nextEnd > start && old[end - 1] === next[nextEnd - 1]) {
          end--;
          nextEnd--;
        }
        if (end !== start || nextEnd !== start) {
          rt.prop(node, "splice", [start, end - start, nextEnd - start]);
        }
      }
      rt.prop(node, "count", next.length);
      state.previous = next;
    });
    // Everything else goes to the list's scroll view (style, testID,
    // events); the list's own props stay here.
    reactive.renderEffect(() => {
      const keys = Object.keys(props).filter((key) => !own.has(key));
      for (const key of state.forwarded.keys()) {
        if (!keys.includes(key)) {
          rt.prop(node, key, undefined);
          state.forwarded.delete(key);
        }
      }
      for (const key of keys) {
        const value = props[key];
        if (!state.forwarded.has(key) || state.forwarded.get(key) !== value) {
          rt.prop(node, key, value);
          state.forwarded.set(key, value);
        }
      }
    });
    for (const [key, fallback] of [
      ["estimatedItemSize", 50],
      ["numColumns", 1],
      ["loadMoreThreshold", 1],
    ]) {
      const value = props[key];
      rt.prop(node, key, typeof value === "number" && value > 0 ? value : fallback);
    }
    // Header, footer and the empty placeholder: non-recycled cells.
    const extra = (role) => {
      const made = root(() => {
        const cell = rt.element("cell");
        rt.prop(cell, "slot", role);
        rt.append(node, cell);
        rt.insert(cell, () => slot(props[role]));
        return cell;
      });
      state.extras.push({ node: made.value, dispose: made.dispose });
      return made.value;
    };
    for (const role of ["header", "footer"]) {
      if (role in props) extra(role);
    }
    if ("empty" in props) extra("empty");
    if (typeof props.initialScrollIndex === "number") {
      invoke(309, id, "scrollToIndex", Math.max(0, props.initialScrollIndex), false);
    }
    if (typeof props.ref === "function") {
      reactive.untracked(() => props.ref(handle(id)));
    } else if (props.ref && typeof props.ref === "object") {
      props.ref.current = handle(id);
    }
    return node;
  }

  function handle(id) {
    return {
      scrollToIndex(index, options = {}) {
        invoke(309, id, "scrollToIndex", Math.max(0, index ?? 0), !!options.animated);
      },
      scrollToEnd(options = {}) {
        invoke(309, id, "scrollToEnd", !!options.animated);
      },
    };
  }

  function bind(id, bindings) {
    const state = lists.get(id);
    if (!state) return;
    reactive.batch(() => {
      for (const [slotId, first] of bindings) {
        if (lists.get(id) !== state) break;
        const existing = state.cells.get(slotId);
        if (existing) {
          existing.index.set(first);
          continue;
        }
        let index;
        const made = root(() => {
          index = reactive.signal(first);
          let previous;
          const item = reactive.computed(() => {
            const items = state.props.each ?? [];
            const i = index();
            if (i < items.length) previous = items[i];
            return previous;
          });
          const cell = rt.element("cell");
          rt.prop(cell, "slot", slotId);
          rt.append(state.node, cell);
          const separator = "separator" in state.props ? slot(state.props.separator) : undefined;
          rt.insert(cell, () => {
            const content = state.props.children(item, index);
            if (separator === undefined) return content;
            const items = state.props.each ?? [];
            return index() >= items.length - 1 ? content : [content, separator];
          });
          return cell;
        });
        state.cells.set(slotId, { node: made.value, index, dispose: made.dispose });
      }
    });
    for (const [slotId, first] of bindings) {
      const cell = state.cells.get(slotId);
      if (cell) rt.prop(cell.node, "cellIndex", first);
    }
    if (lists.get(id) === state) wake();
  }

  return { render, bind };
}
