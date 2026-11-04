//! End-to-end hot module replacement: a development bundle, in-place
//! patches, and a full reload in the same engine.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use nlayer_bundler::{HotBundle, HotPatch, bundle_hot};
use nlayer_core::{
    EventKind, MountBatch, MountItem, NodeId, ROOT_ID,
    geometry::{Size, px},
    text::ApproxTextEngine,
};
use nlayer_engine::{Engine, EngineConfig, EventPayload, HotUpdate, MountSink};

struct HotSink {
    batches: Sender<MountBatch>,
    failed: Sender<()>,
    errors: Sender<String>,
}

impl MountSink for HotSink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.batches.send(batch);
    }

    fn hot_update_failed(&self) {
        let _ = self.failed.send(());
    }

    fn failed(&self, error: nlayer_engine::EngineError) {
        let _ = self.errors.send(error.message);
    }
}

const APP: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";
import { Child } from "./Child";

const kept = signal(0);

function App() {
  return (
    <View style={{ width: 200, height: 80 }}>
      <Text onPress={() => kept.set(kept() + 1)}>{`kept ${kept()}`}</Text>
      <Child />
    </View>
  );
}

mount(App);
"#;

const CHILD: &str = r#"
import { Text } from "@nlayer/ui";
import { label } from "./label";

export function Child() {
  return <Text>{label}</Text>;
}
"#;

const LABEL: &str = r#"export const label = "child";"#;

/// Latest text of each view, and which views are still in the tree.
#[derive(Default)]
struct Screen {
    texts: std::collections::BTreeMap<NodeId, String>,
    parents: std::collections::HashMap<NodeId, NodeId>,
    press: Option<NodeId>,
}

impl Screen {
    fn apply(&mut self, batch: &MountBatch) {
        for item in &batch.items {
            match item {
                MountItem::Text { id, text, .. } => {
                    self.texts.insert(*id, text.text.clone());
                }
                MountItem::Insert { parent, child, .. } => {
                    self.parents.insert(*child, *parent);
                }
                MountItem::Remove { id } | MountItem::Delete { id } => {
                    self.parents.remove(id);
                    if self.press == Some(*id) {
                        self.press = None;
                    }
                }
                MountItem::Events { id, events } if events.contains(EventKind::Press) => {
                    self.press = Some(*id);
                }
                _ => {}
            }
        }
    }

    fn shown(&self) -> Vec<String> {
        let attached = |mut id: NodeId| loop {
            if id == ROOT_ID {
                return true;
            }
            match self.parents.get(&id) {
                Some(&parent) => id = parent,
                None => return false,
            }
        };
        let mut texts: Vec<String> = self
            .texts
            .iter()
            .filter(|(id, _)| attached(**id))
            .map(|(_, text)| text.clone())
            .collect();
        texts.sort();
        texts
    }
}

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nlayer-engine-hot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("App.tsx"), APP).unwrap();
    std::fs::write(dir.join("Child.tsx"), CHILD).unwrap();
    std::fs::write(dir.join("label.ts"), LABEL).unwrap();
    dir
}

fn bundle(dir: &Path) -> HotBundle {
    bundle_hot(&dir.join("App.tsx"), false).unwrap_or_else(|error| panic!("{}", error.plain))
}

struct Running {
    engine: Engine,
    bundle: HotBundle,
    batches: Receiver<MountBatch>,
    failed: Receiver<()>,
    errors: Receiver<String>,
    screen: Screen,
}

fn start(name: &str) -> (PathBuf, Running) {
    let dir = fixture(name);
    let bundled = bundle(&dir);
    let (code, _) = bundled.script();
    let (batches_tx, batches) = crossbeam_channel::unbounded();
    let (failed_tx, failed) = crossbeam_channel::unbounded();
    let (errors_tx, errors) = crossbeam_channel::unbounded();
    let engine = Engine::start(
        EngineConfig::new(code, Size::new(px(390.0), px(844.0))),
        || ApproxTextEngine,
        std::sync::Arc::new(HotSink {
            batches: batches_tx,
            failed: failed_tx,
            errors: errors_tx,
        }),
    )
    .expect("the engine starts");
    let mut running = Running {
        engine,
        bundle: bundled,
        batches,
        failed,
        errors,
        screen: Screen::default(),
    };
    running.drain();
    (dir, running)
}

impl Running {
    fn drain(&mut self) {
        // The first frame is committed before `Engine::start` returns, but
        // wait if it has not been received yet.
        if self.screen.texts.is_empty()
            && let Ok(batch) = self.batches.recv_timeout(Duration::from_secs(5))
        {
            self.screen.apply(&batch);
        }
        while let Ok(batch) = self.batches.try_recv() {
            self.screen.apply(&batch);
        }
    }

    fn settle(&mut self) {
        self.engine
            .control()
            .synchronize()
            .expect("the engine is running");
        self.drain();
    }

    fn press_kept(&self) {
        let id = self.screen.press.expect("a press listener");
        self.engine
            .control()
            .dispatch_event(id, EventKind::Press, EventPayload::None);
    }

    fn hot_update(&mut self, n: u32, dir: &Path) {
        let next = bundle(dir);
        let patch = next.patch(&self.bundle);
        self.bundle = next;
        let update = match patch {
            HotPatch::Update { code, source_map } => HotUpdate {
                name: format!("hot-{n}.js"),
                code,
                source_map,
            },
            other => panic!("expected an update, got {other:?}"),
        };
        self.engine.control().hot_update(update);
        self.settle();
    }
}

#[test]
fn a_child_edit_keeps_an_unchanged_module_signal() {
    let (dir, mut app) = start("keep");
    assert_eq!(app.screen.shown(), ["child", "kept 0"]);

    app.press_kept();
    app.settle();
    assert_eq!(app.screen.shown(), ["child", "kept 1"]);

    std::fs::write(
        dir.join("Child.tsx"),
        CHILD.replace("<Text>{label}</Text>", "<Text>child!</Text>"),
    )
    .unwrap();
    app.hot_update(1, &dir);
    assert_eq!(
        app.screen.shown(),
        ["child!", "kept 1"],
        "the unchanged module's signal stays"
    );
    assert!(app.failed.try_recv().is_err(), "the patch applied");
    assert!(app.errors.try_recv().is_err(), "no error");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_module_without_accept_changes_its_importer() {
    let (dir, mut app) = start("bubble");
    assert_eq!(app.screen.shown(), ["child", "kept 0"]);

    std::fs::write(dir.join("label.ts"), LABEL.replace("child", "bubbled")).unwrap();
    app.hot_update(1, &dir);
    assert_eq!(
        app.screen.shown(),
        ["bubbled", "kept 0"],
        "the importer renders the edited module"
    );
    assert!(
        app.failed.try_recv().is_err(),
        "a bubble that lands is not a reload"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn decline_reloads_the_bundle_and_drops_state() {
    let (dir, mut app) = start("decline");
    app.press_kept();
    app.settle();
    assert_eq!(app.screen.shown(), ["child", "kept 1"]);

    std::fs::write(
        dir.join("label.ts"),
        "import.meta.hot.decline();\nexport const label = \"child\";\n",
    )
    .unwrap();
    app.hot_update(1, &dir);
    assert!(
        app.failed.try_recv().is_ok(),
        "decline asks for a full reload"
    );
    assert_eq!(
        app.screen.shown(),
        ["child", "kept 0"],
        "the reloaded bundle's top-level drops the signal"
    );
    assert!(
        app.errors.try_recv().is_err(),
        "decline is a reload, not an error"
    );

    // The host path for a base mismatch: a new full script, same engine.
    app.press_kept();
    app.settle();
    assert_eq!(app.screen.shown(), ["child", "kept 1"]);
    std::fs::write(dir.join("label.ts"), LABEL.replace("child", "fresh")).unwrap();
    let code = bundle(&dir).script().0;
    app.engine.control().replace_bundle(code);
    app.settle();
    assert_eq!(
        app.screen.shown(),
        ["fresh", "kept 0"],
        "replace_bundle runs the new bundle's top-level in this engine"
    );
    assert!(
        app.failed.try_recv().is_err(),
        "an explicit bundle is not a failure"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_thrown_patch_is_reported_and_keeps_modules_that_ran() {
    let (dir, mut app) = start("throw");
    app.press_kept();
    app.settle();
    assert_eq!(app.screen.shown(), ["child", "kept 1"]);

    app.engine.control().hot_update(HotUpdate {
        name: "hot-throw.js".to_owned(),
        code: "throw new Error(\"broken hot\");".to_owned(),
        source_map: None,
    });
    app.settle();
    let error = app.errors.try_recv().expect("the error is reported");
    assert!(error.contains("broken hot"), "{error}");
    assert!(app.failed.try_recv().is_err(), "a throw is not a reload");
    assert_eq!(
        app.screen.shown(),
        ["child", "kept 1"],
        "modules that already ran stay"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
