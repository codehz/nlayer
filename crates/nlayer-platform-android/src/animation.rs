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
struct Gestures {
    x: Option<AnimationNode>,
    y: Option<AnimationNode>,
    scale: Option<AnimationNode>,
    angle: Option<AnimationNode>,
    /// Values when the gesture began: pan x, pan y, pinch, rotation.
    start: [f32; 4],
}

#[derive(Default)]
struct UiAnimation {
    graph: Graph,
    finished: Vec<Finished>,
    bindings: std::collections::HashMap<NodeId, Vec<(AnimationProp, AnimationNode)>>,
    styles: std::collections::HashMap<NodeId, ViewStyle>,
    gestures: std::collections::HashMap<NodeId, Gestures>,
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
        gestures,
        dirty,
    } = &mut *state;
    for item in items {
        match item {
            MountItem::Animation(op) => {
                graph.apply(op.clone(), finished);
                *dirty = true;
            }
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
            MountItem::PanBinding { id, x, y } => {
                if x.is_none() && y.is_none() {
                    gestures.remove(id);
                } else {
                    let g = gestures.entry(*id).or_default();
                    g.x = *x;
                    g.y = *y;
                }
            }
            MountItem::PinchBinding { id, scale } => {
                if scale.is_none() {
                    if let Some(g) = gestures.get_mut(id) {
                        g.scale = None;
                        if g.x.is_none() && g.y.is_none() && g.angle.is_none() {
                            gestures.remove(id);
                        }
                    }
                } else {
                    gestures.entry(*id).or_default().scale = *scale;
                }
            }
            MountItem::RotationBinding { id, angle } => {
                if angle.is_none() {
                    if let Some(g) = gestures.get_mut(id) {
                        g.angle = None;
                        if g.x.is_none() && g.y.is_none() && g.scale.is_none() {
                            gestures.remove(id);
                        }
                    }
                } else {
                    gestures.entry(*id).or_default().angle = *angle;
                }
            }
            MountItem::Delete { id } => {
                bound.remove(id);
                styles.remove(id);
                gestures.remove(id);
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

/// Whether view `id` has pan gesture bindings.
pub(crate) fn has_pan(id: NodeId) -> bool {
    state()
        .gestures
        .get(&id)
        .is_some_and(|g| g.x.is_some() || g.y.is_some())
}

/// A pan gesture began: read current values and stop animations.
pub(crate) fn pan_began(id: NodeId) {
    let mut s = state();
    let (x_node, y_node) = if let Some(g) = s.gestures.get(&id) {
        (g.x, g.y)
    } else {
        return;
    };
    let x_val = x_node.and_then(|n| s.graph.value(n)).unwrap_or(0.0);
    let y_val = y_node.and_then(|n| s.graph.value(n)).unwrap_or(0.0);
    if let Some(g) = s.gestures.get_mut(&id) {
        g.start[0] = x_val;
        g.start[1] = y_val;
    }
}

/// The drag moved by `(tx, ty)` points since it began.
pub(crate) fn pan_moved(id: NodeId, tx: f32, ty: f32) {
    let mut s = state();
    let (x, y, start) = if let Some(g) = s.gestures.get(&id) {
        (g.x, g.y, g.start)
    } else {
        return;
    };
    let UiAnimation {
        graph, finished, ..
    } = &mut *s;
    if let Some(n) = x {
        graph.set(n, start[0] + tx, finished);
    }
    if let Some(n) = y {
        graph.set(n, start[1] + ty, finished);
    }
}

/// The current pan values for reporting in events.
pub(crate) fn pan_values(id: NodeId) -> (Option<f32>, Option<f32>) {
    let s = state();
    let Some(g) = s.gestures.get(&id) else {
        return (None, None);
    };
    (
        g.x.and_then(|n| s.graph.value(n)),
        g.y.and_then(|n| s.graph.value(n)),
    )
}

/// A pinch or rotation gesture began.
pub(crate) fn transform_began(id: NodeId, rotation: bool) {
    let mut s = state();
    let node = if let Some(g) = s.gestures.get(&id) {
        if rotation { g.angle } else { g.scale }
    } else {
        return;
    };
    let (slot, default) = if rotation { (3, 0.0) } else { (2, 1.0) };
    let val = node.and_then(|n| s.graph.value(n)).unwrap_or(default);
    if let Some(g) = s.gestures.get_mut(&id) {
        g.start[slot] = val;
    }
}

/// The pinch scale (or rotation in degrees) since the gesture began.
pub(crate) fn transform_moved(id: NodeId, rotation: bool, amount: f32) {
    let mut s = state();
    let (node, start_slot) = if let Some(g) = s.gestures.get(&id) {
        if rotation {
            (g.angle, g.start[3])
        } else {
            (g.scale, g.start[2])
        }
    } else {
        return;
    };
    if let Some(n) = node {
        let v = if rotation {
            start_slot + amount
        } else {
            start_slot * amount
        };
        let UiAnimation {
            graph, finished, ..
        } = &mut *s;
        graph.set(n, v, finished);
    }
}

/// The current pinch/rotation value for reporting in events.
pub(crate) fn transform_value(id: NodeId, rotation: bool) -> Option<f32> {
    let s = state();
    let g = s.gestures.get(&id)?;
    let node = if rotation { g.angle } else { g.scale };
    node.and_then(|n| s.graph.value(n))
}
