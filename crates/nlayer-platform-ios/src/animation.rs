//! The animation graph on the main thread, advanced by a `CADisplayLink`
//! that only runs while something animates.
//!
//! The mounter writes the graph's values over each view's static style every
//! frame; the graph itself is platform-free (`nlayer-core`).
//!
//! From `nsh-platform-ios/src/animation.rs`, cut to the v2 view bindings.

use nlayer_core::animation::{AnimationOp, Finished, Graph};

#[derive(Default)]
pub(crate) struct Animator {
    graph: Graph,
    finished: Vec<Finished>,
}

impl Animator {
    pub(crate) fn apply(&mut self, op: AnimationOp) {
        self.graph.apply(op, &mut self.finished);
    }

    pub(crate) fn tick(&mut self, now: f64) -> bool {
        self.graph.tick(now, &mut self.finished)
    }

    pub(crate) fn animating(&self) -> bool {
        self.graph.is_animating()
    }

    pub(crate) fn eval(&self, node: u32) -> nlayer_core::animation::AnimationValue {
        self.graph.eval(node)
    }

    pub(crate) fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }
}
