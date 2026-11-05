//! Dev-server wire protocol (WebSocket at [`WS_PATH`]).
//!
//! Control messages are JSON text frames. A [`Bundle`] text frame is
//! followed by one binary frame of the development script (UTF-8) and, when
//! [`Bundle::source_map`] is set, one binary frame of source-map JSON. A
//! [`Patch`] is framed the same way and is sent before the [`Bundle`] of
//! the same version. Hosts whose running version is not [`Patch::base`]
//! ignore the patch and wait for that bundle.
//!
//! [`Hello::test`] marks a test session: the host must not show the dev
//! overlay and must not apply patches. [`Step`] is one existing test step;
//! the host answers with [`StepResult`]. [`Recycle`] drops the running
//! engine and the native tree, then answers with [`StepResult`] too; the
//! next [`Bundle`] starts a new QuickJS runtime in the same process.

use serde::{Deserialize, Serialize};

/// Protocol version; bump on incompatible changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// WebSocket endpoint path on the dev server.
pub const WS_PATH: &str = "/ws";

macro_rules! frame_tag {
    ($name:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            Tag,
        }
    };
}

frame_tag!(HelloTag, "hello");
frame_tag!(PatchTag, "patch");
frame_tag!(BundleTag, "bundle");
frame_tag!(BuildFailedTag, "build_failed");
frame_tag!(RecycleTag, "recycle");
frame_tag!(StepTag, "step");
frame_tag!(StepResultTag, "step_result");

/// Server hello. Test sessions set [`Self::test`] so the host skips the
/// dev overlay and does not apply patches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    #[serde(rename = "type")]
    tag: HelloTag,
    /// [`PROTOCOL_VERSION`] of the server.
    pub protocol: u32,
    /// This connection is a test session, not `nlayer run`.
    pub test: bool,
}

impl Hello {
    /// A hello for a run session (`test == false`) or a test session.
    pub fn new(test: bool) -> Self {
        Self {
            tag: HelloTag::Tag,
            protocol: PROTOCOL_VERSION,
            test,
        }
    }
}

/// Hot update from [`Self::base`] to [`Self::version`].
///
/// The next binary frame is the script (UTF-8 JS). When [`Self::source_map`]
/// is set, a second binary frame is the source map (JSON). The [`Bundle`] of
/// `version` follows either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    #[serde(rename = "type")]
    tag: PatchTag,
    /// Build number after the update.
    pub version: u64,
    /// Build number the update applies to.
    pub base: u64,
    /// A second binary frame with the source map follows.
    #[serde(default)]
    pub source_map: bool,
}

impl Patch {
    /// A patch frame. `source_map` is whether the source-map binary frame follows.
    pub fn new(version: u64, base: u64, source_map: bool) -> Self {
        Self {
            tag: PatchTag::Tag,
            version,
            base,
            source_map,
        }
    }
}

/// A full development bundle. The next binary frame is the script (UTF-8).
/// When [`Self::source_map`] is set, a second binary frame is the source map.
///
/// New connections receive the current bundle. There is no capabilities
/// list, VM limits, app id, or bytecode format on this frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(rename = "type")]
    tag: BundleTag,
    /// Monotonic build number. The first successful build is 1.
    pub version: u64,
    /// A second binary frame with the source map follows.
    #[serde(default)]
    pub source_map: bool,
}

impl Bundle {
    /// A bundle frame. `source_map` is whether the source-map binary frame follows.
    pub fn new(version: u64, source_map: bool) -> Self {
        Self {
            tag: BundleTag::Tag,
            version,
            source_map,
        }
    }
}

/// Safe area on a [`Recycle`], in points.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RecycleInsets {
    /// Distance from the top of the screen, in points.
    #[serde(default)]
    pub top: f32,
    /// Distance from the right of the screen, in points.
    #[serde(default)]
    pub right: f32,
    /// Distance from the bottom of the screen, in points.
    #[serde(default)]
    pub bottom: f32,
    /// Distance from the left of the screen, in points.
    #[serde(default)]
    pub left: f32,
}

/// Drop the engine and the native tree, then wait for the next [`Bundle`].
///
/// Answered with [`StepResult`] of [`Self::id`]. The process and the test
/// socket stay. A bundle that arrives afterwards is a new QuickJS runtime,
/// not a reload of the one this frame dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recycle {
    #[serde(rename = "type")]
    tag: RecycleTag,
    /// Correlates with the [`StepResult`] that says the tree is gone.
    pub id: u64,
    /// Viewport width, in points.
    pub width: f32,
    /// Viewport height, in points.
    pub height: f32,
    /// Pinned `env(keyboard-inset-height)`. `0` is a pinned zero, not "follow
    /// the software keyboard": a test pin always replaces it.
    #[serde(default)]
    pub keyboard: f32,
    /// Pinned safe area.
    #[serde(default, rename = "safeArea")]
    pub safe_area: RecycleInsets,
    /// `light` or `dark`. Absent keeps the scheme the process already has.
    #[serde(
        default,
        rename = "colorScheme",
        skip_serializing_if = "Option::is_none"
    )]
    pub color_scheme: Option<String>,
}

impl Recycle {
    /// A recycle of viewport `width`×`height`. Insets and the scheme are
    /// filled in by the caller.
    pub fn new(id: u64, width: f32, height: f32) -> Self {
        Self {
            tag: RecycleTag::Tag,
            id,
            width,
            height,
            keyboard: 0.0,
            safe_area: RecycleInsets::default(),
            color_scheme: None,
        }
    }
}

/// The build failed. `run` hosts show `message` and keep the previous app.
/// A test session fails `open` instead of showing the dev overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildFailed {
    #[serde(rename = "type")]
    tag: BuildFailedTag,
    /// Compiler diagnostic, plain text.
    pub message: String,
}

impl BuildFailed {
    /// A failed build with `message` as the diagnostic.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            tag: BuildFailedTag::Tag,
            message: message.into(),
        }
    }
}

/// Names of the steps `@nlayer/test` already drives.
///
/// Serialized as the `op` string [`crate`] hosts send today (`longPress`,
/// `dragAndHold`, `waitFor`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepOp {
    /// Every text view's content.
    Texts,
    /// Every input's value.
    Inputs,
    /// Press what [`Step::selector`] names.
    Press,
    /// Hover what the selector names.
    Hover,
    /// Focus what the selector names.
    Focus,
    /// Hold a press without firing it.
    Down,
    /// Release the press [`Self::Down`] held.
    Up,
    /// Class warnings so far.
    Warnings,
    /// Long-press what the selector names.
    LongPress,
    /// Type [`Step::text`] into the selector.
    Type,
    /// Report a selection. The range travels in [`Step::text`] until M6
    /// splits it; `x` and `y` are unused.
    Select,
    /// Report a composition ([`Step::text`]).
    Compose,
    /// Scroll by [`Step::distance`] points.
    Scroll,
    /// Drag by ([`Step::x`], [`Step::y`]) points.
    Drag,
    /// Drag by (`x`, `y`) and keep the pointer down.
    DragAndHold,
    /// Release the held pointer.
    Drop,
    /// Navigation back.
    Back,
    /// Advance the test clock by [`Step::ms`] milliseconds.
    Advance,
    /// Wait until the selector matches a view on screen.
    WaitFor,
    /// The mounted tree.
    Tree,
    /// Which view owns the selection of the selector.
    SelectionOwner,
    /// URLs opened by `href` presses, in order.
    OpenedUrls,
    /// Close the app.
    Close,
}

/// One test step. The selector fields are the ones `nlayer-cli`'s hosted
/// apps send today (`op`, `selector`, `text`, `distance`, `x`, `y`, `ms`),
/// plus [`Self::id`] so the host can answer with [`StepResult`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    #[serde(rename = "type")]
    tag: StepTag,
    /// Correlates with [`StepResult::id`].
    pub id: u64,
    /// Which step to run.
    pub op: StepOp,
    /// View selector (`id:save`, `text:Add`, …). Empty when the step has none.
    #[serde(default)]
    pub selector: String,
    /// Typed text, composition, or other text argument. Empty when unused.
    #[serde(default)]
    pub text: String,
    /// Scroll distance in points.
    #[serde(default)]
    pub distance: f32,
    /// Horizontal drag, in points.
    #[serde(default)]
    pub x: f32,
    /// Vertical drag, in points.
    #[serde(default)]
    pub y: f32,
    /// Clock advance, in milliseconds.
    #[serde(default)]
    pub ms: f64,
}

impl Step {
    /// A step `id` of kind `op` with empty selector fields.
    pub fn new(id: u64, op: StepOp) -> Self {
        Self {
            tag: StepTag::Tag,
            id,
            op,
            selector: String::new(),
            text: String::new(),
            distance: 0.0,
            x: 0.0,
            y: 0.0,
            ms: 0.0,
        }
    }
}

/// How a [`Step`] ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    #[serde(rename = "type")]
    tag: StepResultTag,
    /// The [`Step::id`] this answers.
    pub id: u64,
    /// The step finished without an error.
    pub ok: bool,
    /// Set when [`Self::ok`] is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the step read (`texts`, `tree`, a `waitFor` state, …). Absent
    /// for steps that only act.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

impl StepResult {
    /// The step `id` finished.
    pub fn ok(id: u64) -> Self {
        Self {
            tag: StepResultTag::Tag,
            id,
            ok: true,
            error: None,
            value: None,
        }
    }

    /// The step `id` finished, reading `value`.
    pub fn done(id: u64, value: serde_json::Value) -> Self {
        Self {
            tag: StepResultTag::Tag,
            id,
            ok: true,
            error: None,
            value: Some(value),
        }
    }

    /// The step `id` failed with `message`.
    pub fn error(id: u64, message: impl Into<String>) -> Self {
        Self {
            tag: StepResultTag::Tag,
            id,
            ok: false,
            error: Some(message.into()),
            value: None,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn messages_roundtrip() {
        let hello = Hello::new(false);
        let hello_json = serde_json::to_string(&hello).unwrap();
        assert_eq!(hello_json, r#"{"type":"hello","protocol":1,"test":false}"#);
        assert_eq!(serde_json::from_str::<Hello>(&hello_json).unwrap(), hello);
        let test_hello = Hello::new(true);
        assert!(test_hello.test);
        assert_eq!(
            serde_json::from_str::<Hello>(&serde_json::to_string(&test_hello).unwrap()).unwrap(),
            test_hello
        );

        let patch = Patch::new(2, 1, true);
        let patch_json = serde_json::to_string(&patch).unwrap();
        assert_eq!(
            patch_json,
            r#"{"type":"patch","version":2,"base":1,"source_map":true}"#
        );
        assert_eq!(serde_json::from_str::<Patch>(&patch_json).unwrap(), patch);

        let bundle = Bundle::new(2, false);
        let bundle_json = serde_json::to_string(&bundle).unwrap();
        assert_eq!(
            bundle_json,
            r#"{"type":"bundle","version":2,"source_map":false}"#
        );
        assert_eq!(
            serde_json::from_str::<Bundle>(&bundle_json).unwrap(),
            bundle
        );

        let failed = BuildFailed::new("App.tsx:1:1");
        let failed_json = serde_json::to_string(&failed).unwrap();
        assert_eq!(
            failed_json,
            r#"{"type":"build_failed","message":"App.tsx:1:1"}"#
        );
        assert_eq!(
            serde_json::from_str::<BuildFailed>(&failed_json).unwrap(),
            failed
        );
        let mut recycle = Recycle::new(4, 390.0, 844.0);
        recycle.keyboard = 120.0;
        recycle.safe_area.top = 47.0;
        recycle.color_scheme = Some("dark".to_owned());
        let recycle_json = serde_json::to_string(&recycle).unwrap();
        assert_eq!(
            recycle_json,
            r#"{"type":"recycle","id":4,"width":390.0,"height":844.0,"keyboard":120.0,"safeArea":{"top":47.0,"right":0.0,"bottom":0.0,"left":0.0},"colorScheme":"dark"}"#
        );
        assert_eq!(
            serde_json::from_str::<Recycle>(&recycle_json).unwrap(),
            recycle
        );

        let mut step = Step::new(7, StepOp::LongPress);
        step.selector = "text:Go".to_owned();
        step.distance = 12.5;
        let step_json = serde_json::to_string(&step).unwrap();
        assert!(step_json.contains(r#""type":"step""#), "{step_json}");
        assert!(step_json.contains(r#""op":"longPress""#), "{step_json}");
        assert!(step_json.contains(r#""selector":"text:Go""#), "{step_json}");
        assert!(step_json.contains(r#""id":7"#), "{step_json}");
        assert_eq!(serde_json::from_str::<Step>(&step_json).unwrap(), step);

        let ok = StepResult::ok(7);
        let ok_json = serde_json::to_string(&ok).unwrap();
        assert_eq!(ok_json, r#"{"type":"step_result","id":7,"ok":true}"#);
        assert_eq!(serde_json::from_str::<StepResult>(&ok_json).unwrap(), ok);

        let err = StepResult::error(7, "matches nothing");
        let err_json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            err_json,
            r#"{"type":"step_result","id":7,"ok":false,"error":"matches nothing"}"#
        );
        assert_eq!(serde_json::from_str::<StepResult>(&err_json).unwrap(), err);

        let names = [
            (StepOp::Texts, "texts"),
            (StepOp::Inputs, "inputs"),
            (StepOp::Press, "press"),
            (StepOp::Hover, "hover"),
            (StepOp::Focus, "focus"),
            (StepOp::Down, "down"),
            (StepOp::Up, "up"),
            (StepOp::Warnings, "warnings"),
            (StepOp::LongPress, "longPress"),
            (StepOp::Type, "type"),
            (StepOp::Select, "select"),
            (StepOp::Compose, "compose"),
            (StepOp::Scroll, "scroll"),
            (StepOp::Drag, "drag"),
            (StepOp::DragAndHold, "dragAndHold"),
            (StepOp::Drop, "drop"),
            (StepOp::Back, "back"),
            (StepOp::Advance, "advance"),
            (StepOp::WaitFor, "waitFor"),
            (StepOp::Tree, "tree"),
            (StepOp::SelectionOwner, "selectionOwner"),
            (StepOp::OpenedUrls, "openedUrls"),
            (StepOp::Close, "close"),
        ];
        for (op, wire) in names {
            let json = serde_json::to_string(&op).unwrap();
            assert_eq!(json, format!("\"{wire}\""));
            assert_eq!(serde_json::from_str::<StepOp>(&json).unwrap(), op);
        }
    }
}
