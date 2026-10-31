//! The native animation graph on the browser's main thread.
//!
//! Copied from `nsh-platform-web/src/animation.rs` (baseline ca59368b).
//! Gesture value tracking (`transform_began`, `transform_moved`, `pan_began`,
//! `pan_moved`) drives animated values from pointer input.

use nlayer_core::{
    MountItem,
    animation::{AnimationOp, Graph, NodeId},
};
use rustc_hash::FxHashMap;

#[derive(Default)]
struct Gestures {
    /// Pan x, pan y, pinch scale, rotation angle node ids.
    nodes: [Option<NodeId>; 4],
    /// Values when the gesture began: pan x, pan y, pinch, rotation.
    start: [f32; 4],
}

#[derive(Default)]
pub(crate) struct UiAnimation {
    graph: Graph,
    gestures: FxHashMap<u32, Gestures>,
}

impl UiAnimation {
    /// Takes the animation items of a batch; returns true if the item was one.
    pub(crate) fn apply(&mut self, item: &MountItem) -> bool {
        match item {
            MountItem::Animation(op) => {
                self.graph.apply(op.clone(), &mut Vec::new());
                true
            }
            MountItem::PanBinding { id, x, y } => {
                if x.is_none() && y.is_none() {
                    if let Some(g) = self.gestures.get_mut(id) {
                        g.nodes[0] = None;
                        g.nodes[1] = None;
                    }
                } else {
                    let g = self.gestures.entry(*id).or_default();
                    g.nodes[0] = *x;
                    g.nodes[1] = *y;
                }
                true
            }
            MountItem::PinchBinding { id, scale } => {
                let g = self.gestures.entry(*id).or_default();
                g.nodes[2] = *scale;
                true
            }
            MountItem::RotationBinding { id, angle } => {
                let g = self.gestures.entry(*id).or_default();
                g.nodes[3] = *angle;
                true
            }
            MountItem::Delete { id } => {
                self.gestures.remove(id);
                false
            }
            _ => false,
        }
    }

    pub(crate) fn tick(&mut self, now: f64) -> bool {
        self.graph.tick(now, &mut Vec::new())
    }

    pub(crate) fn animating(&self) -> bool {
        self.graph.is_animating()
    }

    /// A drag began: grabbing an animating value stops it where it is.
    pub(crate) fn pan_began(&mut self, id: u32) {
        let graph = &self.graph;
        if let Some(g) = self.gestures.get_mut(&id) {
            g.start[0] = g.nodes[0].and_then(|n| graph.value(n)).unwrap_or(0.0);
            g.start[1] = g.nodes[1].and_then(|n| graph.value(n)).unwrap_or(0.0);
        }
    }

    /// The drag moved by `(tx, ty)` points since it began.
    pub(crate) fn pan_moved(&mut self, id: u32, tx: f32, ty: f32) {
        let Some(g) = self.gestures.get(&id) else {
            return;
        };
        let (x, y, start) = (g.nodes[0], g.nodes[1], g.start);
        if let Some(n) = x {
            self.graph.set(n, start[0] + tx, &mut Vec::new());
        }
        if let Some(n) = y {
            self.graph.set(n, start[1] + ty, &mut Vec::new());
        }
    }

    /// The values the drag on `id` moves (`pan.x`, `pan.y`).
    pub(crate) fn pan_values(&self, id: u32) -> (Option<f32>, Option<f32>) {
        let Some(g) = self.gestures.get(&id) else {
            return (None, None);
        };
        (
            g.nodes[0].and_then(|n| self.graph.value(n)),
            g.nodes[1].and_then(|n| self.graph.value(n)),
        )
    }

    /// A pinch (`rotation` false) or two-finger rotation began.
    pub(crate) fn transform_began(&mut self, id: u32, rotation: bool) {
        let graph = &self.graph;
        if let Some(g) = self.gestures.get_mut(&id) {
            let (node, slot, default) = if rotation {
                (g.nodes[3], 3, 0.0)
            } else {
                (g.nodes[2], 2, 1.0)
            };
            g.start[slot] = node.and_then(|n| graph.value(n)).unwrap_or(default);
        }
    }

    /// The pinch scale (or rotation in degrees) since the gesture began.
    pub(crate) fn transform_moved(&mut self, id: u32, rotation: bool, amount: f32) {
        let Some(g) = self.gestures.get(&id) else {
            return;
        };
        if rotation {
            if let Some(n) = g.nodes[3] {
                let v = g.start[3] + amount;
                self.graph.set(n, v, &mut Vec::new());
            }
        } else if let Some(n) = g.nodes[2] {
            let v = g.start[2] * amount;
            self.graph.set(n, v, &mut Vec::new());
        }
    }

    /// Whether view `id` has any gesture bindings.
    pub(crate) fn has_gestures(&self, id: u32) -> (bool, bool, bool) {
        let Some(g) = self.gestures.get(&id) else {
            return (false, false, false);
        };
        let pan = g.nodes[0].is_some() || g.nodes[1].is_some();
        let pinch = g.nodes[2].is_some();
        let rotation = g.nodes[3].is_some();
        (pan, pinch, rotation)
    }
}
