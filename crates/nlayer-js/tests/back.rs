//! `BackHandler`: listeners run newest first, and a `true` stops the walk.

use nlayer_js::Host;

#[test]
fn the_newest_listener_handles_first() {
    let host = Host::new().expect("a VM");
    host.eval_source(
        r#"
import { BackHandler } from "@nlayer/ui";

globalThis.order = "";
globalThis.removeOlder = BackHandler.addEventListener(() => { order += "older"; return true; });
globalThis.removeNewer = BackHandler.addEventListener(() => { order += "newer"; return true; });
"#,
    )
    .expect("the listeners are added");

    assert!(host.dispatch_back());
    assert_eq!(host.global_string("order").as_deref(), Some("newer"));

    host.eval_source(
        r#"
order = "";
removeNewer();
"#,
    )
    .expect("the newer listener is removed");
    assert!(host.dispatch_back());
    assert_eq!(host.global_string("order").as_deref(), Some("older"));

    host.eval_source(
        r#"
order = "";
removeOlder();
"#,
    )
    .expect("the older listener is removed");
    assert!(!host.dispatch_back());
    assert_eq!(host.global_string("order").as_deref(), Some(""));
}

#[test]
fn the_color_scheme_is_set_before_the_app_runs() {
    let host = Host::new().expect("a VM");
    host.set_color_scheme(nlayer_core::ColorScheme::Dark);
    host.eval_source(
        r#"
import { platform } from "@nlayer/ui";
globalThis.scheme = platform.colorScheme();
"#,
    )
    .expect("the app runs");
    assert_eq!(host.global_string("scheme").as_deref(), Some("dark"));
}
