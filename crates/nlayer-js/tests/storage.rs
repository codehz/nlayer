//! `storage` of `@nlayer/runtime`: promises over the core's entries.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use nlayer_core::storage::{StorageBackend, StorageChange, Store};
use nlayer_js::Host;

/// A backend keeping the entries in memory and recording the changes.
#[derive(Default)]
struct Recorder {
    kept: BTreeMap<String, String>,
    changes: Mutex<Vec<StorageChange>>,
}

impl StorageBackend for Recorder {
    fn load(&self) -> Result<BTreeMap<String, String>, String> {
        Ok(self.kept.clone())
    }

    fn write(&self, change: StorageChange) {
        self.changes.lock().unwrap().push(change);
    }
}

fn host_with(backend: Arc<Recorder>) -> Host {
    let host = Host::new().unwrap();
    host.set_storage(Store::new(Some(backend)));
    host
}

fn run(host: &Host, source: &str) -> Result<(), String> {
    host.eval_source(source).map_err(|error| error.message)
}

#[test]
fn reads_and_writes_are_promises_and_reach_the_backend() {
    let backend = Arc::new(Recorder::default());
    let host = host_with(backend.clone());
    run(
        &host,
        r#"
import { storage } from "@nlayer/runtime";

if (!(storage.getItem("k") instanceof Promise)) throw new Error("getItem is not a promise");
if (await storage.getItem("k") !== null) throw new Error("missing key is not null");
await storage.setItem("b", "2");
await storage.setItem("a", "1");
const keys = await storage.keys();
if (JSON.stringify(keys) !== '["a","b"]') throw new Error(JSON.stringify(keys));
if (await storage.getItem("a") !== "1") throw new Error("read back");
await storage.removeItem("a");
if ((await storage.keys()).length !== 1) throw new Error("remove");
await storage.clear();
if ((await storage.keys()).length !== 0) throw new Error("clear");
"#,
    )
    .unwrap();
    assert_eq!(
        *backend.changes.lock().unwrap(),
        [
            StorageChange::Set("b".into(), "2".into()),
            StorageChange::Set("a".into(), "1".into()),
            StorageChange::Remove("a".into()),
            StorageChange::Clear,
        ]
    );
}

#[test]
fn an_existing_backend_is_read_before_the_first_write() {
    let backend = Arc::new(Recorder {
        kept: BTreeMap::from([("draft".into(), "saved".into())]),
        ..Recorder::default()
    });
    let host = host_with(backend.clone());
    run(
        &host,
        r#"
import { storage } from "@nlayer/runtime";
if (await storage.getItem("draft") !== "saved") throw new Error("not loaded");
await storage.setItem("draft", "new");
"#,
    )
    .unwrap();
    assert_eq!(
        *backend.changes.lock().unwrap(),
        [StorageChange::Set("draft".into(), "new".into())]
    );
}

#[test]
fn a_write_past_the_quota_rejects_and_keeps_the_old_value() {
    let host = Host::new().unwrap();
    run(
        &host,
        r#"
import { storage } from "@nlayer/runtime";

await storage.setItem("k", "small");
try {
    await storage.setItem("k", "x".repeat(5 * 1024 * 1024));
    throw new Error("accepted a value past the quota");
} catch (error) {
    if (error.name !== "QuotaExceededError") throw new Error(`name ${error.name}`);
    if (error.code !== "ERR_NLAYER_QUOTA_EXCEEDED") throw new Error(`code ${error.code}`);
}
if (await storage.getItem("k") !== "small") throw new Error("the old value went");
"#,
    )
    .unwrap();
}

#[test]
fn keys_and_values_are_strings() {
    let host = Host::new().unwrap();
    run(
        &host,
        r#"
import { storage } from "@nlayer/runtime";
await storage.setItem(1, 2);
if (await storage.getItem("1") !== "2") throw new Error(await storage.getItem("1"));
"#,
    )
    .unwrap();
}

#[test]
fn two_hosts_on_one_backend_share_entries() {
    let backend = Arc::new(Recorder::default());
    let first = host_with(backend.clone());
    run(
        &first,
        r#"
import { storage } from "@nlayer/runtime";
await storage.setItem("draft", "hello");
"#,
    )
    .unwrap();
    let second = host_with(backend);
    run(
        &second,
        r#"
import { storage } from "@nlayer/runtime";
if (await storage.getItem("draft") !== "hello") throw new Error("not shared");
"#,
    )
    .unwrap();
}
