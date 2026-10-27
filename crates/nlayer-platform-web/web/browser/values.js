// Values the wasm bridge names. Slots 0–3 are the constants. A generation
// in the high bits stops a stale id from seeing a recycled slot.
const INDEX_BITS = 20;
const INDEX_MASK = (1 << INDEX_BITS) - 1;
const MAX_GENERATION = (1 << (32 - INDEX_BITS)) - 1;
export const EXCEPTION = 0xffffffff;

export class Values {
  constructor() {
    this.slots = [undefined, null, false, true].map((value) => ({
      value,
      refs: Infinity,
      generation: 0,
    }));
    this.freeSlots = [];
    this.closed = false;
    this.error = null;
    this.hasError = false;
    this.nodes = new Map();
    this.nodeIds = new WeakMap();
    this.makeNode = null;
    this.depth = 0;
  }

  put(value) {
    if (this.closed) throw new Error("the app was disposed");
    if (value === undefined) return 0;
    if (value === null) return 1;
    if (value === false) return 2;
    if (value === true) return 3;
    const index = this.freeSlots.pop() ?? this.slots.length;
    if (index >= INDEX_MASK) throw new RangeError("too many retained browser values");
    const generation = this.slots[index]?.generation ?? 0;
    this.slots[index] = {
      value,
      refs: 1,
      generation,
      node: this.nodeIds.get(value) ?? 0,
    };
    return ((generation << INDEX_BITS) | index) >>> 0;
  }

  slot(id) {
    if (this.closed) throw new Error("the app was disposed");
    const slot = this.slots[id & INDEX_MASK];
    if (!slot || slot.refs === 0 || slot.generation !== id >>> INDEX_BITS) {
      throw new ReferenceError("a browser value was released");
    }
    return slot;
  }

  get(id) {
    return this.slot(id >>> 0).value;
  }

  retain(id) {
    this.slot(id).refs++;
    return id;
  }

  release(id) {
    if (this.closed || id < 4) return;
    const slot = this.slot(id);
    if (--slot.refs !== 0) return;
    slot.value = undefined;
    if (slot.generation < MAX_GENERATION) {
      slot.generation++;
      this.freeSlots.push(id & INDEX_MASK);
    }
  }

  with(values, body) {
    const ids = [];
    try {
      for (const value of values) ids.push(this.put(value));
      return body(ids);
    } finally {
      for (let i = ids.length - 1; i >= 0; i--) this.release(ids[i]);
    }
  }

  result(id) {
    if ((id >>> 0) === EXCEPTION) {
      const error = this.error;
      const pending = this.hasError;
      this.error = null;
      this.hasError = false;
      throw pending ? error : new Error("the browser host failed");
    }
    try {
      return this.get(id);
    } finally {
      this.release(id);
    }
  }

  boundary(body) {
    this.depth++;
    try {
      return body();
    } finally {
      this.depth--;
    }
  }

  imports(memory) {
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    const bytes = (ptr, length) => new Uint8Array(memory().buffer, ptr, length);
    const text = (ptr, length) => decoder.decode(bytes(ptr, length));
    return {
      value: (op, a, b, c, number) => {
        try {
          switch (op) {
            case 0: {
              const v = this.get(a);
              if (v === undefined) return 0;
              if (v === null) return 1;
              if (typeof v === "boolean") return 2;
              if (typeof v === "number") return 3;
              if (typeof v === "string") return 4;
              if (Array.isArray(v)) return 5;
              if (typeof v === "function") return 6;
              return typeof v === "object" ? 7 : 8;
            }
            case 1:
              return typeof this.get(a) === "boolean" ? Number(this.get(a)) : NaN;
            case 2:
              return typeof this.get(a) === "number" ? this.get(a) : NaN;
            case 3:
              return typeof this.get(a) === "string" ? encoder.encode(this.get(a)).length : NaN;
            case 4:
              bytes(b, c).set(encoder.encode(this.get(a)));
              return 0;
            case 7:
              return Array.isArray(this.get(a)) ? this.get(a).length : NaN;
            case 8:
              return this.put(this.get(a)[b]);
            case 9:
              return this.put(this.get(a)?.[text(b, c)]);
            case 10:
              return this.put(Object.keys(this.get(a)));
            case 14:
              return this.slot(a).node ?? 0;
            case 16:
              return this.retain(a);
            case 17:
              this.release(a);
              return 0;
            case 18:
              return this.put(number);
            case 19:
              return this.put(text(b, c));
            case 21:
              return this.put([]);
            case 22:
              this.get(a)[b] = this.get(c);
              this.release(c);
              return 0;
            case 23:
              return this.put({});
            case 26:
              return this.put(Reflect.apply(this.get(a), this.get(b), this.get(c)));
            case 32:
              return Number(this.get(a) === this.get(b));
            case 38: {
              let message = "browser value operation failed";
              const error = this.error;
              if (typeof error === "string") message = error;
              else if (typeof error?.message === "string") message = error.message;
              this.error = null;
              this.hasError = false;
              return this.put(message);
            }
            case 39:
              return this.put(this.makeNode(a, b !== 0));
            case 43:
              return this.put(String(this.get(a)));
            default:
              throw new Error(`unknown browser value operation ${op}`);
          }
        } catch (error) {
          this.error = error;
          this.hasError = true;
          return EXCEPTION;
        }
      },
    };
  }
}
