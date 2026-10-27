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
