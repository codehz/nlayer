//! `resource` of `@nlayer/runtime`: loads, superseding, errors, `refetch`
//! and `mutate`.

use nlayer_js::Host;

fn run(source: &str) {
    Host::new().unwrap().eval_source(source).unwrap();
}

#[test]
fn loads_the_source_and_reads_the_value() {
    run(r#"
import { signal, resource } from "@nlayer/runtime";

const id = signal(undefined);
const calls = [];
const user = resource(id, (value, info) => {
    calls.push(value);
    if (info.signal.aborted) throw new Error("aborted before start");
    return Promise.resolve(`user ${value}`);
});
if (user.state() !== "unresolved") throw new Error(user.state());
if (user() !== undefined) throw new Error("a value before a load");
if (calls.length !== 0) throw new Error("loaded without a source");

id.set(7);
if (user.state() !== "pending" || !user.loading()) throw new Error(user.state());
await Promise.resolve();
await Promise.resolve();
if (user() !== "user 7") throw new Error(String(user()));
if (user.state() !== "ready" || user.loading()) throw new Error(user.state());
if (user.error() !== undefined) throw new Error("an error after success");
if (JSON.stringify(calls) !== "[7]") throw new Error(JSON.stringify(calls));
"#);
}

#[test]
fn a_source_change_supersedes_and_aborts_the_previous_load() {
    run(r#"
import { signal, resource } from "@nlayer/runtime";

const id = signal(1);
const aborts = [];
const resolvers = [];
const user = resource(id, (value, info) => {
    info.signal.addEventListener("abort", () => aborts.push(value));
    return new Promise((resolve) => resolvers.push(() => resolve(`user ${value}`)));
});
id.set(2);
id.set(3);
await Promise.resolve();
// Only the latest load counts.
resolvers[0]();
resolvers[1]();
resolvers[2]();
await Promise.resolve();
await Promise.resolve();
if (user() !== "user 3") throw new Error(String(user()));
if (user.state() !== "ready") throw new Error(user.state());
if (JSON.stringify(aborts) !== "[1,2]") throw new Error(JSON.stringify(aborts));
"#);
}

#[test]
fn a_failure_keeps_the_value_and_reports_the_error() {
    run(r#"
import { signal, resource } from "@nlayer/runtime";

const fail = signal(false);
const user = resource(() => new Promise((resolve, reject) => {
    if (fail()) reject(new Error("down"));
    else resolve("ok");
}));
await Promise.resolve();
if (user() !== "ok") throw new Error(String(user()));
fail.set(true);
user.refetch();
if (user.state() !== "refreshing") throw new Error(user.state());
await Promise.resolve();
await Promise.resolve();
if (user.state() !== "errored") throw new Error(user.state());
if (user() !== "ok") throw new Error("the value went");
if (!String(user.error()).includes("down")) throw new Error(String(user.error()));
if (user.loading()) throw new Error("still loading");
"#);
}

#[test]
fn refetch_reports_refetching_and_mutate_sets_the_value() {
    run(r#"
import { resource } from "@nlayer/runtime";

const seen = [];
const user = resource((info) => {
    seen.push(info.refetching);
    return Promise.resolve("first");
});
await Promise.resolve();
await Promise.resolve();
user.refetch();
if (user.state() !== "refreshing") throw new Error(user.state());
await Promise.resolve();
await Promise.resolve();
user.mutate("set here");
if (user() !== "set here") throw new Error(String(user()));
if (user.state() !== "ready") throw new Error(user.state());
if (JSON.stringify(seen) !== "[false,true]") throw new Error(JSON.stringify(seen));
"#);
}

#[test]
fn a_fetcher_that_throws_errored_and_a_false_source_stays_unresolved() {
    run(r#"
import { signal, resource } from "@nlayer/runtime";

const source = signal(0);
const user = resource(source, (value) => {
    if (value === 0) return undefined;
    throw new TypeError("bad source");
});
if (user.state() !== "unresolved") throw new Error(user.state());
source.set(1);
if (user.state() !== "errored") throw new Error(user.state());
if (!String(user.error()).includes("bad source")) throw new Error(String(user.error()));
source.set(0);
if (user.state() !== "unresolved") throw new Error(user.state());
"#);
}

#[test]
fn a_resource_object_is_callable_and_keeps_its_methods() {
    run(r#"
import { resource } from "@nlayer/runtime";

const user = resource(() => Promise.resolve(1));
const state = user.state;
const loading = user.loading;
await Promise.resolve();
await Promise.resolve();
if (typeof user !== "function") throw new Error(typeof user);
if (user() !== 1) throw new Error(String(user()));
if (state() !== "ready") throw new Error(state());
if (loading() !== false) throw new Error("loading");
"#);
}
