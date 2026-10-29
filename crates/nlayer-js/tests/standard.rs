//! The globals an app finds without importing anything.

use nlayer_js::Host;

fn run(source: &str) {
    Host::new().unwrap().eval_source(source).unwrap();
}

#[test]
fn text_encoder_and_decoder_round_trip() {
    run(r#"
        const bytes = new TextEncoder().encode("héllo 🌍");
        if (!(bytes instanceof Uint8Array)) throw new Error("not a Uint8Array");
        if (bytes.length !== 11) throw new Error(`length ${bytes.length}`);
        const text = new TextDecoder().decode(bytes);
        if (text !== "héllo 🌍") throw new Error(text);
        // An ArrayBuffer is bytes too.
        const copy = bytes.buffer.slice(0);
        if (new TextDecoder().decode(copy) !== text) throw new Error("array buffer");
    "#);
}

/// Encoding's `decode()`: no input, a `DataView`, a BOM, and invalid UTF-8.
#[test]
fn text_decoder_accepts_missing_and_view_input() {
    run(r#"
        const dec = new TextDecoder();
        if (dec.decode() !== "") throw new Error("no argument");
        if (dec.decode(undefined) !== "") throw new Error("undefined");
        const view = new DataView(new Uint8Array([0x78, 0x6f, 0x6b, 0x79]).buffer, 1, 2);
        if (dec.decode(view) !== "ok") throw new Error(dec.decode(view));
        const bom = dec.decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61]));
        if (bom !== "a") throw new Error(JSON.stringify(bom));
        const bad = dec.decode(new Uint8Array([0x61, 0xff, 0x62]));
        if (bad !== "a\uFFFDb") throw new Error(JSON.stringify(bad));
        try {
            dec.decode("nope");
            throw new Error("accepted a string");
        } catch (e) {
            if (!(e instanceof TypeError) || !String(e).includes("ArrayBuffer")) throw e;
        }
    "#);
}

#[test]
fn base64_and_url() {
    run(r#"
        if (btoa("hello") !== "aGVsbG8=") throw new Error(btoa("hello"));
        if (atob("aGVsbG8=") !== "hello") throw new Error(atob("aGVsbG8="));
        const url = new URL("https://example.com/a?b=1");
        if (url.pathname !== "/a") throw new Error(url.pathname);
        if (url.searchParams.get("b") !== "1") throw new Error("query");
        const params = new URLSearchParams({ a: "1", b: "2" });
        if (params.toString() !== "a=1&b=2") throw new Error(params.toString());
    "#);
}

/// `searchParams` rewrites the URL, and `search` / `href` parse it again.
#[test]
fn url_search_params_stay_live() {
    run(r##"
        const url = new URL("/a?b=1", "https://example.com/base/x");
        if (url.href !== "https://example.com/a?b=1") throw new Error(url.href);
        if (url.origin !== "https://example.com") throw new Error(url.origin);
        if (String(url) !== url.href || url.toJSON() !== url.href) throw new Error("string");
        url.searchParams.append("c", "2");
        if (url.search !== "?b=1&c=2") throw new Error(url.search);
        url.search = "?z=9";
        if (url.searchParams.get("z") !== "9" || url.searchParams.get("b") !== null) {
            throw new Error(url.searchParams.toString());
        }
        url.href = "https://example.com/q?m=1#h";
        if (url.hash !== "#h" || url.searchParams.get("m") !== "1") throw new Error(url.href);
        if (!URL.canParse("/c", "https://example.com/a/")) throw new Error("canParse relative");
        if (!URL.canParse("https://example.com/") || URL.canParse("not a url")) {
            throw new Error("canParse");
        }
        if (URL.parse("not a url") !== null) throw new Error("parse failed");
        const parsed = URL.parse("c?d=1", "https://example.com/base/x");
        if (!(parsed instanceof URL) || parsed.href !== "https://example.com/base/c?d=1") {
            throw new Error(String(parsed));
        }
        try { new URL("not a url"); throw new Error("constructor accepted it"); }
        catch (e) { if (!(e instanceof TypeError)) throw e; }

        const params = new URLSearchParams([["b", "2"], ["a", "1"]]);
        params.sort();
        if (params.toString() !== "a=1&b=2" || params.size !== 2) throw new Error(params.toString());
        params.set("a", "0");
        params.append("a", "3");
        if (params.getAll("a").join(",") !== "0,3") throw new Error(params.getAll("a").join(","));
        params.delete("a", "0");
        if (!params.has("a", "3") || params.has("a", "0")) throw new Error(params.toString());
        if (new URLSearchParams("a=1+2").get("a") !== "1 2") throw new Error("plus");
        const seen = [];
        params.forEach(function (value, key) { seen.push(this + key + value); }, "T");
        if (seen.join("|") !== "Tb2|Ta3") throw new Error(seen.join("|"));
    "##);
}

#[test]
fn performance_and_random_values() {
    run(r#"
        const t = performance.now();
        if (typeof t !== "number" || !(t >= 0)) throw new Error(String(t));
        const a = new Uint8Array(16);
        crypto.getRandomValues(a);
        const b = new Uint8Array(16);
        crypto.getRandomValues(b);
        if (a.every((v) => v === 0)) throw new Error("all zero");
        if (a.every((v, i) => v === b[i])) throw new Error("same twice");
    "#);
}

#[test]
fn microtasks_run_after_the_turn() {
    run(r#"
        const order = [];
        queueMicrotask(() => order.push("micro"));
        order.push("sync");
        await Promise.resolve();
        if (order.join(",") !== "sync,micro") throw new Error(order.join(","));
    "#);
}
