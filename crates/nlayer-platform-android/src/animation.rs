//! The animation graph on the Android UI thread.
//!
//! `Choreographer` calls `Native.tick`; the Rust graph moves and returns one
//! encoded `ViewStyle` batch for the bound views, which the Java mounter
//! applies. This keeps the graph off the engine thread, as `nsh` does.

use std::sync::{Mutex, MutexGuard, OnceLock};

use nlayer_core::{
    MountBatch, MountItem, NodeId,
    animation::{AnimationProp, Finished, Graph, NodeId as AnimationNode},
    style::ViewStyle,
};

#[derive(Default)]
struct UiAnimation {
    graph: Graph,
    finished: Vec<Finished>,
    bindings: std::collections::HashMap<NodeId, Vec<(AnimationProp, AnimationNode)>>,
    styles: std::collections::HashMap<NodeId, ViewStyle>,
    /// Bindings changed since the last frame: paint once even without a
    /// running animation.
    dirty: bool,
}

fn state() -> MutexGuard<'static, UiAnimation> {
    static STATE: OnceLock<Mutex<UiAnimation>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(UiAnimation::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Forgets the running app's graph (a new app runs).
pub(crate) fn reset() {
    *state() = UiAnimation::default();
}

/// Takes the animation items of a batch; other items keep going to the
/// mounter. Also remembers each view's static style.
pub(crate) fn note(items: &[MountItem]) {
    let mut state = state();
    let UiAnimation {
        graph,
        finished,
        bindings: bound,
        styles,
        dirty,
    } = &mut *state;
    for item in items {
        match item {
            MountItem::Animation(op) => graph.apply(op.clone(), finished),
            MountItem::AnimationBindings { id, bindings } => {
                if bindings.is_empty() {
                    bound.remove(id);
                } else {
                    bound.insert(*id, bindings.clone());
                }
                *dirty = true;
            }
            MountItem::ViewStyle { id, style } => {
                styles.insert(*id, (**style).clone());
            }
            MountItem::Delete { id } => {
                bound.remove(id);
                styles.remove(id);
            }
            _ => {}
        }
    }
}

/// Whether the graph runs, or a binding still needs its first paint.
pub(crate) fn animating() -> bool {
    let state = state();
    state.dirty || state.graph.is_animating()
}

/// Advances to `now` seconds and returns the styles of the bound views.
pub(crate) fn frame(now: f64) -> MountBatch {
    let mut state = state();
    state.dirty = false;
    {
        let UiAnimation {
            graph, finished, ..
        } = &mut *state;
        graph.tick(now, finished);
    }
    let mut items = Vec::with_capacity(state.bindings.len());
    for (&id, bindings) in &state.bindings {
        let mut style = state.styles.get(&id).cloned().unwrap_or_default();
        for &(prop, node) in bindings {
            let value = state.graph.eval(node);
            match prop {
                AnimationProp::Opacity => style.opacity = value.number(),
                AnimationProp::BackgroundColor => {
                    style.background_color = nlayer_core::style::Color(value.color())
                }
                AnimationProp::BorderColor => {
                    style.borders.colors = [nlayer_core::style::Color(value.color()); 4]
                }
                AnimationProp::Transform(i) => {
                    if style.transform.ops().get(i as usize).is_some() {
                        style.transform.set(i, value.number());
                    }
                }
            }
        }
        items.push(MountItem::ViewStyle {
            id,
            style: Box::new(style),
        });
    }
    MountBatch { items }
}

/// Animations that finished or were interrupted since the last call.
pub(crate) fn take_finished() -> Vec<Finished> {
    std::mem::take(&mut state().finished)
}
