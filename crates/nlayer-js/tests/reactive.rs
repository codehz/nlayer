//! The reactive graph as the app module `@nlayer` exposes it.

use nlayer_js::Host;

/// `@nlayer`'s exports, driven from a module.
#[test]
fn signals_are_read_by_calling_and_written_with_set_and_update() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { signal, computed, effect, batch, untracked } from "@nlayer";

        const count = signal(0);
        const doubled = computed(() => count() * 2);
        const log = [];
        effect(() => log.push(`${count()}/${doubled()}`));
        count.set(1);
        count.update((n) => n + 2);
        batch(() => {
            count.set(10);
            count.set(11);
        });
        const untrackedValue = untracked(() => count());
        if (untrackedValue !== 11) throw new Error(`untracked: ${untrackedValue}`);
        if (log.join(",") !== "0/0,1/2,3/6,11/22") throw new Error(log.join(","));
        "#,
    )
    .unwrap();
}

/// `cleanup(fn)` runs `fn` before its effect runs again and when the app
/// goes away.
#[test]
fn cleanup_runs_before_each_rerun() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { signal, effect, cleanup } from "@nlayer";

        const count = signal(0);
        const log = [];
        effect(() => {
            count();
            cleanup(() => log.push("cleanup"));
            log.push("run");
        });
        count.set(1);
        count.set(2);
        if (log.join(",") !== "run,cleanup,run,cleanup,run") throw new Error(log.join(","));
        "#,
    )
    .unwrap();
}

/// An import of a module the runtime does not serve fails the bundle.
#[test]
fn an_unknown_native_module_is_an_error() {
    let host = Host::new().unwrap();
    let error = host
        .eval_source("import * as nsh from 'nsh';")
        .expect_err("nsh is not a module here");
    assert!(
        error.message.contains("could not load module 'nsh'"),
        "{}",
        error.message
    );
}

/// A bundle that throws fails `eval_source` with the thrown message.
#[test]
fn a_thrown_error_is_reported() {
    let host = Host::new().unwrap();
    let error = host
        .eval_source("throw new Error('boom');")
        .expect_err("the bundle throws");
    assert!(error.message.contains("boom"), "{}", error.message);
}
