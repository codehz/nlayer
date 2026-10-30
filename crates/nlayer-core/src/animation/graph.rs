//! The animation graph: values, interpolations and the drivers moving them.
//!
//! JS declares the graph; a host frame clock calls [`Graph::tick`]. Evaluation
//! is on demand and memoized per change, so a view binding only reads the
//! nodes it uses. The only view state read back is through [`Finished`].
//!
//! Extracted from `nsh-core/src/animation/graph.rs` and cut to the v2 graph:
//! no composites, diffClamp, math nodes or offsets.

use std::cell::RefCell;

use super::{
    driver::{Running, ease},
    ops::{
        AnimationId, AnimationOp, AnimationValue, Driver, Easing, Extrapolate, Finished, NodeId,
        Output,
    },
};

#[derive(Debug)]
enum Node {
    Value {
        value: f32,
        velocity: f32,
        running: Option<Running>,
    },
    Interpolate {
        input: NodeId,
        input_range: Vec<f32>,
        output: Output,
        extrapolate: (Extrapolate, Extrapolate),
        easing: Easing,
        /// Its breakpoints ascend (none below the one before, no NaN):
        /// a segment is found by bisection.
        ascending: bool,
    },
}

/// Nodes deeper than this are treated as cycles (JS bug) and evaluate to 0.
const MAX_DEPTH: u32 = 64;

/// All animation nodes of one surface.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: Vec<Option<Node>>,
    /// Value nodes with a running animation.
    animating: Vec<NodeId>,
    /// Bumped on every change; memoized values of older generations are stale.
    generation: u64,
    values: RefCell<Vec<(u64, AnimationValue)>>,
    /// Evaluation hit a cycle: do not memoize the outer value.
    cut: std::cell::Cell<bool>,
}

impl Graph {
    /// Applies an op, reporting the animations it interrupts.
    pub fn apply(&mut self, op: AnimationOp, finished: &mut Vec<Finished>) {
        self.changed();
        self.apply_op(op, finished);
        self.changed();
    }

    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn apply_op(&mut self, op: AnimationOp, finished: &mut Vec<Finished>) {
        match op {
            AnimationOp::CreateValue { node, value } => {
                self.insert(
                    node,
                    Node::Value {
                        value,
                        velocity: 0.0,
                        running: None,
                    },
                );
            }
            AnimationOp::CreateInterpolate {
                node,
                input,
                input_range,
                output,
                extrapolate_left,
                extrapolate_right,
                easing,
            } => {
                let n = input_range.len().min(output_len(&output));
                let ascending = input_range
                    .windows(2)
                    .all(|w| w[0] <= w[1] && !w[0].is_nan() && !w[1].is_nan());
                self.insert(
                    node,
                    Node::Interpolate {
                        input,
                        input_range: input_range.into_iter().take(n).collect(),
                        output: truncate(output, n),
                        extrapolate: (extrapolate_left, extrapolate_right),
                        easing,
                        ascending,
                    },
                );
            }
            AnimationOp::Set { node, value } => {
                self.stop(node, finished);
                if let Some(Some(Node::Value {
                    value: current,
                    velocity,
                    ..
                })) = self.nodes.get_mut(node as usize)
                {
                    *current = value;
                    *velocity = 0.0;
                    self.animating.retain(|&n| n != node);
                }
            }
            AnimationOp::Start {
                node,
                animation,
                driver,
            } => {
                self.start(node, animation, driver, finished);
            }
            AnimationOp::Stop { node } => {
                self.stop(node, finished);
                if let Some(Some(Node::Value { velocity, .. })) = self.nodes.get_mut(node as usize)
                {
                    *velocity = 0.0;
                }
                self.animating.retain(|&n| n != node);
            }
            AnimationOp::Release { node } => {
                self.stop(node, finished);
                self.animating.retain(|&n| n != node);
                if let Some(slot) = self.nodes.get_mut(node as usize) {
                    *slot = None;
                }
            }
            AnimationOp::Read { node, request } => {
                finished.push(Finished {
                    animation: request,
                    finished: true,
                    value: Some(self.eval(node).number()),
                });
            }
        }
    }

    fn insert(&mut self, id: NodeId, node: Node) {
        let i = id as usize;
        if self.nodes.len() <= i {
            self.nodes.resize_with(i + 1, || None);
        }
        self.nodes[i] = Some(node);
        self.animating.retain(|&n| n != id);
    }

    fn start(
        &mut self,
        node: NodeId,
        animation: AnimationId,
        driver: Driver,
        finished: &mut Vec<Finished>,
    ) {
        let Some(Some(Node::Value {
            value,
            velocity,
            running,
        })) = self.nodes.get_mut(node as usize)
        else {
            return;
        };
        // The value keeps its velocity through an interruption, so a spring
        // replacing another starts smoothly.
        let new = Running::new(animation, driver, *value, *velocity);
        if let Some(old) = running.replace(new) {
            finished.push(Finished::new(old.id, false));
        }
        if !self.animating.contains(&node) {
            self.animating.push(node);
        }
    }

    fn stop(&mut self, node: NodeId, finished: &mut Vec<Finished>) {
        let Some(Some(Node::Value {
            running, velocity, ..
        })) = self.nodes.get_mut(node as usize)
        else {
            return;
        };
        if let Some(running) = running.take() {
            finished.push(Finished::new(running.id, false));
        }
        *velocity = 0.0;
    }

    /// Sets a value immediately (`AnimatedValue.set`), stopping its animation.
    pub fn set(&mut self, node: NodeId, value: f32, finished: &mut Vec<Finished>) {
        self.changed();
        self.apply_op(AnimationOp::Set { node, value }, finished);
        self.changed();
    }

    /// The current number of a value node.
    pub fn value(&self, node: NodeId) -> Option<f32> {
        match self.nodes.get(node as usize)?.as_ref()? {
            Node::Value { value, .. } => Some(*value),
            Node::Interpolate { .. } => Some(self.eval(node).number()),
        }
    }

    /// Whether any animation still runs.
    pub fn is_animating(&self) -> bool {
        !self.animating.is_empty()
    }

    /// Advances every running animation to `now` (seconds, monotonic).
    /// Completed animations are pushed to `finished`. Returns true if a
    /// value changed.
    pub fn tick(&mut self, now: f64, finished: &mut Vec<Finished>) -> bool {
        self.changed();
        let mut changed = false;
        let mut i = 0;
        while i < self.animating.len() {
            let node = self.animating[i];
            let (moved, done) = self.step_node(node, now, finished);
            changed |= moved;
            if done {
                self.animating.swap_remove(i);
            } else {
                i += 1;
            }
        }
        self.changed();
        changed
    }

    fn step_node(&mut self, node: NodeId, now: f64, finished: &mut Vec<Finished>) -> (bool, bool) {
        let (v, vel, done) = match self.nodes.get_mut(node as usize) {
            Some(Some(Node::Value {
                running: Some(running),
                ..
            })) => running.step(now),
            _ => return (false, true),
        };
        let Some(Node::Value {
            value,
            velocity,
            running,
            ..
        }) = self.nodes[node as usize].as_mut()
        else {
            return (false, true);
        };
        let changed = *value != v;
        *value = v;
        *velocity = vel;
        if done && let Some(ended) = running.take() {
            finished.push(Finished::new(ended.id, true));
        }
        (changed, done)
    }

    /// Evaluates any node.
    pub fn eval(&self, node: NodeId) -> AnimationValue {
        self.eval_depth(node, 0)
    }

    fn eval_depth(&self, node: NodeId, depth: u32) -> AnimationValue {
        if depth > MAX_DEPTH {
            self.cut.set(true);
            return AnimationValue::Number(0.0);
        }
        let i = node as usize;
        if let Some(&(generation, value)) = self.values.borrow().get(i)
            && generation == self.generation
        {
            return value;
        }
        let outer = self.cut.replace(false);
        let value = self.compute(node, depth);
        if !self.cut.get() {
            let mut values = self.values.borrow_mut();
            if i >= values.len() {
                values.resize(
                    self.nodes.len().max(i + 1),
                    (u64::MAX, AnimationValue::Number(0.0)),
                );
            }
            values[i] = (self.generation, value);
        }
        self.cut.set(outer || self.cut.get());
        value
    }

    fn compute(&self, node: NodeId, depth: u32) -> AnimationValue {
        match self.nodes.get(node as usize) {
            Some(Some(Node::Value { value, .. })) => AnimationValue::Number(*value),
            Some(Some(Node::Interpolate {
                input,
                input_range,
                output,
                extrapolate,
                easing,
                ascending,
            })) => interpolate(
                self.eval_depth(*input, depth + 1),
                input_range,
                output,
                *extrapolate,
                *easing,
                *ascending,
            ),
            _ => AnimationValue::Number(0.0),
        }
    }
}

fn truncate(output: Output, n: usize) -> Output {
    match output {
        Output::Numbers(mut values) => {
            values.truncate(n);
            Output::Numbers(values)
        }
        Output::Colors(mut values) => {
            values.truncate(n);
            Output::Colors(values)
        }
    }
}

fn output_len(output: &Output) -> usize {
    match output {
        Output::Numbers(values) => values.len(),
        Output::Colors(values) => values.len(),
    }
}

fn interpolate(
    input: AnimationValue,
    input_range: &[f32],
    output: &Output,
    extrapolate: (Extrapolate, Extrapolate),
    easing: Easing,
    ascending: bool,
) -> AnimationValue {
    let x = input.number();
    let n = input_range.len().min(output_len(output));
    if n == 0 {
        return AnimationValue::Number(x);
    }
    let first = input_range[0];
    let last = input_range[n - 1];
    let side = if x < first {
        Some((extrapolate.0, 0))
    } else if x > last {
        Some((extrapolate.1, n - 1))
    } else {
        None
    };
    if let Some((mode, at)) = side {
        match mode {
            Extrapolate::Clamp => return at_output(output, at),
            Extrapolate::Extend => {}
        }
    }
    if n == 1 {
        return at_output(output, 0);
    }
    let segment = if ascending {
        input_range[1..n].partition_point(|v| *v < x).min(n - 2)
    } else {
        input_range[1..n]
            .iter()
            .position(|v| x <= *v)
            .unwrap_or(n - 2)
    };
    let a = input_range[segment];
    let b = input_range[segment + 1];
    let mut t = if b == a { 0.0 } else { (x - a) / (b - a) };
    if !matches!(easing, Easing::Linear) && (0.0..=1.0).contains(&t) {
        t = ease(easing, t);
    }
    match output {
        Output::Numbers(values) => {
            let from = values[segment];
            let to = values[segment + 1];
            AnimationValue::Number(from + (to - from) * t)
        }
        Output::Colors(values) => {
            AnimationValue::Color(lerp_color(values[segment], values[segment + 1], t))
        }
    }
}

fn at_output(output: &Output, at: usize) -> AnimationValue {
    match output {
        Output::Numbers(values) => AnimationValue::Number(values[at]),
        Output::Colors(values) => AnimationValue::Color(values[at]),
    }
}

fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let mut out = 0u32;
    for shift in [24, 16, 8, 0] {
        let ca = (a >> shift) & 0xff;
        let cb = (b >> shift) & 0xff;
        let c = (ca as f32 + (cb as f32 - ca as f32) * t).round() as u32;
        out |= (c & 0xff) << shift;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::ops::StepPosition;

    fn of_value(value: f32, ops: impl IntoIterator<Item = AnimationOp>) -> Graph {
        let mut g = Graph::default();
        let mut finished = Vec::new();
        g.apply(AnimationOp::CreateValue { node: 1, value }, &mut finished);
        for op in ops {
            g.apply(op, &mut finished);
        }
        g
    }

    fn timing(to: f32, ms: f32) -> Driver {
        Driver::Timing {
            to,
            duration_ms: ms,
            easing: Easing::Linear,
            delay_ms: 0.0,
        }
    }

    #[test]
    fn a_value_starts_where_js_made_it() {
        let g = of_value(3.0, []);
        assert_eq!(g.eval(1), AnimationValue::Number(3.0));
        assert_eq!(g.value(1), Some(3.0));
    }

    #[test]
    fn a_timing_animation_moves_and_finishes_once() {
        let mut g = of_value(
            0.0,
            [AnimationOp::Start {
                node: 1,
                animation: 2,
                driver: timing(10.0, 1000.0),
            }],
        );
        let mut finished = Vec::new();
        assert!(!g.tick(10.0, &mut finished));
        assert_eq!(g.value(1), Some(0.0));
        g.tick(10.5, &mut finished);
        assert_eq!(g.value(1), Some(5.0));
        assert!(g.is_animating());
        assert!(g.tick(11.0, &mut finished));
        assert_eq!(g.value(1), Some(10.0));
        assert!(!g.is_animating());
        assert_eq!(finished, [Finished::new(2, true)]);
        assert!(!g.tick(12.0, &mut finished));
        assert_eq!(finished.len(), 1);
    }

    #[test]
    fn a_new_animation_interrupts_the_old_one_where_it_is() {
        let mut g = of_value(
            0.0,
            [AnimationOp::Start {
                node: 1,
                animation: 2,
                driver: timing(10.0, 1000.0),
            }],
        );
        let mut finished = Vec::new();
        g.tick(10.0, &mut finished);
        let before = g.value(1);
        g.apply(
            AnimationOp::Start {
                node: 1,
                animation: 3,
                driver: timing(20.0, 1000.0),
            },
            &mut finished,
        );
        assert_eq!(finished, [Finished::new(2, false)]);
        g.tick(10.0, &mut finished);
        g.tick(10.5, &mut finished);
        assert_eq!(g.value(1), Some(before.unwrap() + 10.0));
        assert!(g.is_animating());
    }

    #[test]
    fn stopping_keeps_the_current_value_and_velocity() {
        let mut g = of_value(
            0.0,
            [AnimationOp::Start {
                node: 1,
                animation: 2,
                driver: timing(10.0, 1000.0),
            }],
        );
        let mut finished = Vec::new();
        g.tick(10.0, &mut finished);
        g.tick(10.2, &mut finished);
        g.apply(AnimationOp::Stop { node: 1 }, &mut finished);
        assert_eq!(finished, [Finished::new(2, false)]);
        assert_eq!(g.value(1), Some(2.0));
        assert!(!g.is_animating());
    }

    #[test]
    fn release_finishes_a_running_animation_unfinished() {
        let mut g = of_value(
            0.0,
            [AnimationOp::Start {
                node: 1,
                animation: 2,
                driver: timing(10.0, 1000.0),
            }],
        );
        let mut finished = Vec::new();
        g.apply(AnimationOp::Release { node: 1 }, &mut finished);
        assert_eq!(finished, [Finished::new(2, false)]);
        assert_eq!(g.value(1), None);
        assert!(!g.is_animating());
    }

    #[test]
    fn a_read_answers_with_the_evaluated_value() {
        let mut g = of_value(4.5, []);
        let mut finished = Vec::new();
        g.apply(
            AnimationOp::Read {
                node: 1,
                request: 9,
            },
            &mut finished,
        );
        assert_eq!(
            finished,
            [Finished {
                animation: 9,
                finished: true,
                value: Some(4.5),
            }]
        );
    }

    #[test]
    fn interpolations_clamp_extend_and_handle_colors() {
        let mut finished = Vec::new();
        let mut g = Graph::default();
        g.apply(
            AnimationOp::CreateValue {
                node: 1,
                value: 0.5,
            },
            &mut finished,
        );
        g.apply(
            AnimationOp::CreateInterpolate {
                node: 2,
                input: 1,
                input_range: vec![0.0, 1.0],
                output: Output::Numbers(vec![0.0, 10.0]),
                extrapolate_left: Extrapolate::Clamp,
                extrapolate_right: Extrapolate::Extend,
                easing: Easing::Linear,
            },
            &mut finished,
        );
        assert_eq!(g.eval(2), AnimationValue::Number(5.0));
        g.set(1, -1.0, &mut finished);
        assert_eq!(g.eval(2), AnimationValue::Number(0.0));
        g.set(1, 2.0, &mut finished);
        assert_eq!(g.eval(2), AnimationValue::Number(20.0));
        g.apply(
            AnimationOp::CreateInterpolate {
                node: 3,
                input: 1,
                input_range: vec![0.0, 2.0],
                output: Output::Colors(vec![0x000000ff, 0xffffffff]),
                extrapolate_left: Extrapolate::Clamp,
                extrapolate_right: Extrapolate::Clamp,
                easing: Easing::Linear,
            },
            &mut finished,
        );
        assert_eq!(g.eval(3), AnimationValue::Color(0xffffffff));
    }

    #[test]
    fn easing_steps_reach_both_ends() {
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpEnd), 0.0), 0.0);
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpEnd), 1.0), 1.0);
    }
}
