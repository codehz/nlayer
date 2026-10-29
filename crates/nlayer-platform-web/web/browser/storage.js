// The app's key-value storage on the page: entries of `localStorage` under
// one prefix per app, with the core's 5 MiB quota (UTF-8 bytes of keys and
// values). The TS API is the same as the QuickJS host's (`storage` of
// `@nlayer/runtime`), so an app reads and writes one set of data wherever
// it runs.

/** The most an app may keep, in UTF-8 bytes of its keys and values. */
export const QUOTA = 5 * 1024 * 1024;

const encoder = new TextEncoder();

/** The `localStorage` prefix of an app: its id, else the page's directory. */
export function storagePrefix(appId, url) {
  if (typeof appId === "string" && appId !== "" && !appId.includes(":")) {
    return `nlayer:${appId}:`;
  }
  const raw = String(url ?? "").split(/[?#]/)[0];
  const scheme = raw.indexOf("://");
  const path =
    scheme >= 0
      ? (() => {
          const rest = raw.slice(scheme + 3);
          const slash = rest.indexOf("/");
          return slash >= 0 ? rest.slice(slash) : "/";
        })()
      : raw;
  const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "/";
  return `nlayer:${dir}:`;
}

function quotaError() {
  const error = new Error(`storage is full: the value would exceed the quota of ${QUOTA} bytes`);
  error.name = "QuotaExceededError";
  error.code = "ERR_NLAYER_QUOTA_EXCEEDED";
  return error;
}

function utf8(text) {
  return encoder.encode(text).length;
}

/** Compares by UTF-8 bytes, the order the core's entries have. */
function compare(a, b) {
  const x = encoder.encode(a);
  const y = encoder.encode(b);
  const n = Math.min(x.length, y.length);
  for (let i = 0; i < n; i++) {
    if (x[i] !== y[i]) return x[i] - y[i];
  }
  return x.length - y.length;
}

/**
 * The app's storage over `area` (a `Storage`, or a stand-in with its
 * methods): keys `prefix + key`.
 */
export function createStorage(area, prefix) {
  function used() {
    let total = 0;
    for (let i = 0; i < area.length; i++) {
      const key = area.key(i);
      if (key === null || !key.startsWith(prefix)) continue;
      const value = area.getItem(key);
      total += utf8(key.slice(prefix.length)) + utf8(value ?? "");
    }
    return total;
  }
  return {
    async getItem(key) {
      return area.getItem(prefix + String(key));
    },
    async setItem(key, value) {
      key = String(key);
      value = String(value);
      const storageKey = prefix + key;
      const old = area.getItem(storageKey);
      const next = used() - (old === null ? 0 : utf8(key) + utf8(old)) + utf8(key) + utf8(value);
      if (next > QUOTA) throw quotaError();
      area.setItem(storageKey, value);
    },
    async removeItem(key) {
      area.removeItem(prefix + String(key));
    },
    async keys() {
      const out = [];
      for (let i = 0; i < area.length; i++) {
        const key = area.key(i);
        if (key === null || !key.startsWith(prefix)) continue;
        out.push(key.slice(prefix.length));
      }
      return out.sort(compare);
    },
    async clear() {
      const keys = [];
      for (let i = 0; i < area.length; i++) {
        const key = area.key(i);
        if (key !== null && key.startsWith(prefix)) keys.push(key);
      }
      for (const key of keys) area.removeItem(key);
    },
  };
}
