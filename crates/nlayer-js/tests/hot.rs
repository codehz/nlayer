// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
//! Hot module replacement (`nlayer:hot`, internal): factories, kept values,
//! component proxies, the accept/bubble rules and the reload flag.
//!
//! A development bundle's first script is an ES module that imports
//! `nlayer:hot` (for the global) and defines its modules; updates are
//! classic scripts whose completion value is `update`'s boolean.

use nlayer_js::Host;

/// Runs an update script; its completion value is the boolean `update`
/// returned.
fn update(host: &Host, source: &str) -> bool {
    host.eval_script("update.js", source)
        .expect("the update runs")
}

/// Evaluates a script that must complete with `true`.
fn check(host: &Host, source: &str) {
    assert!(
        host.eval_script("check.js", source).unwrap_or(false),
        "check failed: {source}"
    );
}

/// The entry factory runs once when `start` runs it, and again on an update.
#[test]
fn start_runs_the_entry_once() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.runs = 0;
        __nlayer_hot.define("entry.js", function (module, exports, require) {
            globalThis.runs++;
            exports.value = 1;
        });
        __nlayer_hot.start("entry.js", {});
        "#,
    )
    .unwrap();
    check(&host, "runs === 1");
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "entry.js": function (module, exports, require) { globalThis.runs++; }
        });"#,
    ));
    check(&host, "runs === 2");
}

/// `keep` returns the stored value while the signature matches, and the
/// new one when it changes.
#[test]
fn keep_reuses_a_value_while_the_signature_matches() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.runs = 0;
        globalThis.sig = "a";
        __nlayer_hot.define("entry.js", function (module, exports, require) {
            globalThis.runs++;
            globalThis.kept = __nlayer_hot.keep(
                "entry.js", "state", globalThis.sig, { run: globalThis.runs });
        });
        __nlayer_hot.start("entry.js", {});
        globalThis.first = globalThis.kept;
        "#,
    )
    .unwrap();
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "entry.js": function (module, exports, require) {
                globalThis.runs++;
                globalThis.kept = __nlayer_hot.keep(
                    "entry.js", "state", globalThis.sig, { run: globalThis.runs });
            }
        });"#,
    ));
    check(&host, "kept === first && kept.run === 1");
    host.eval_script("sig.js", r#"globalThis.sig = "b"; true"#)
        .unwrap();
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "entry.js": function (module, exports, require) {
                globalThis.runs++;
                globalThis.kept = __nlayer_hot.keep(
                    "entry.js", "state", globalThis.sig, { run: globalThis.runs });
            }
        });"#,
    ));
    check(&host, "kept !== first && kept.run === 3");
}

/// `component` returns the same proxy for the same module and name; a new
/// signature renders the instances it already made again.
#[test]
fn component_proxies_are_stable_and_rerender_on_a_new_signature() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.value = 1;
        __nlayer_hot.define("entry.js", function (module, exports, require) {
            exports.Counter = __nlayer_hot.component(
                "entry.js", "Counter", function () { return globalThis.value; }, "sig1");
            globalThis.Counter = exports.Counter;
        });
        __nlayer_hot.start("entry.js", {});
        "#,
    )
    .unwrap();
    check(
        &host,
        r#"__nlayer_hot.component(
            "entry.js", "Counter",
            function () { return globalThis.value; }, "sig1") === Counter"#,
    );
    // A proxy call returns a memo of the implementation now.
    check(&host, "Counter()() === 1");
    host.eval_script("hold.js", "globalThis.held = Counter(); true")
        .unwrap();
    check(&host, "held() === 1");
    host.eval_script(
        "change.js",
        r#"__nlayer_hot.component(
            "entry.js", "Counter",
            function () { return 2; }, "sig2"); true"#,
    )
    .unwrap();
    // The instances made before the change render again, and so does a new call.
    check(&host, "held() === 2");
    check(&host, "Counter()() === 2");
}

/// A module that accepts itself reruns; its importer does not.
#[test]
fn accept_keeps_the_importer_from_rerunning() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.midRuns = 0;
        globalThis.appRuns = 0;
        __nlayer_hot.define("mid.js", function (module, exports, require) {
            globalThis.midRuns++;
            __nlayer_hot.meta("mid.js").accept();
            exports.value = globalThis.midRuns;
        });
        __nlayer_hot.define("app.js", function (module, exports, require) {
            globalThis.appRuns++;
            exports.value = require("./mid.js").value;
        });
        __nlayer_hot.start("app.js", {});
        "#,
    )
    .unwrap();
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "mid.js": function (module, exports, require) {
                globalThis.midRuns++;
                __nlayer_hot.meta("mid.js").accept();
                exports.value = globalThis.midRuns;
            }
        });"#,
    ));
    check(&host, "midRuns === 2 && appRuns === 1");
}

/// Without an accept, the change bubbles to the importer.
#[test]
fn a_module_without_accept_reruns_its_importer() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.midRuns = 0;
        globalThis.appRuns = 0;
        __nlayer_hot.define("mid.js", function (module, exports, require) {
            globalThis.midRuns++;
            exports.value = globalThis.midRuns;
        });
        __nlayer_hot.define("app.js", function (module, exports, require) {
            globalThis.appRuns++;
            exports.value = require("./mid.js").value;
        });
        __nlayer_hot.start("app.js", {});
        "#,
    )
    .unwrap();
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "mid.js": function (module, exports, require) {
                globalThis.midRuns++;
                exports.value = globalThis.midRuns;
            }
        });"#,
    ));
    check(&host, "midRuns === 2 && appRuns === 2");
}

/// `decline` makes the update fail and asks the host for a full reload.
#[test]
fn decline_fails_the_update_and_requests_a_reload() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.runs = 0;
        __nlayer_hot.define("entry.js", function (module, exports, require) {
            globalThis.runs++;
            __nlayer_hot.meta("entry.js").decline();
        });
        __nlayer_hot.start("entry.js", {});
        "#,
    )
    .unwrap();
    assert!(!update(
        &host,
        r#"__nlayer_hot.update({
            "entry.js": function (module, exports, require) { globalThis.runs++; }
        });"#,
    ));
    // The declined factory did not run again.
    check(&host, "runs === 1");
    assert!(host.take_hot_reload(), "decline sets the reload flag");
    assert!(!host.take_hot_reload(), "the flag is cleared once read");
}

/// `dispose` runs before the next run of the factory, and `data` survives.
#[test]
fn dispose_runs_before_the_next_run_and_data_survives() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import "nlayer:hot";
        globalThis.log = [];
        __nlayer_hot.define("entry.js", function (module, exports, require) {
            const hot = __nlayer_hot.meta("entry.js");
            globalThis.log.push("run");
            globalThis.seen = hot.data.seen;
            hot.dispose(function (data) {
                data.seen = "yes";
                globalThis.log.push("dispose");
            });
        });
        __nlayer_hot.start("entry.js", {});
        "#,
    )
    .unwrap();
    check(&host, "log.join(',') === 'run' && seen === undefined");
    assert!(update(
        &host,
        r#"__nlayer_hot.update({
            "entry.js": function (module, exports, require) {
                const hot = __nlayer_hot.meta("entry.js");
                globalThis.log.push("run");
                globalThis.seen = hot.data.seen;
                hot.dispose(function (data) {
                    data.seen = "yes";
                    globalThis.log.push("dispose");
                });
            }
        });"#,
    ));
    check(&host, "log.join(',') === 'run,dispose,run'");
    check(&host, "seen === 'yes'");
}
