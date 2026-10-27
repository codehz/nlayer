//! Host nodes for compiled JSX. A mounted node's page object stays reachable
//! because its parent holds that [`Value`](super::values::Value), as nsh's
//! browser host does. The finalizer releases a node only after it leaves the
//! tree. This table records parents and emits the core's ops.

use std::cell::{Cell, RefCell};

use nlayer_core::{EventKind, NodeId, Op, ROOT_ID, Tag};
use rustc_hash::FxHashMap;

use super::{
    PropError, Runtime,
    protocol::{self, Warnings},
    values::{Args, Value},
};

const DETACHED: NodeId = 0;
const MAX_DEPTH: u32 = 64;

struct Child {
    id: NodeId,
    /// Keeps the page object alive while the node is mounted.
    value: Value,
}

struct Node {
    text: bool,
    tag: Option<Tag>,
    parent: NodeId,
    children: Vec<Child>,
    listening: Vec<EventKind>,
}

enum Item {
    Node(Value),
    Text(String),
}

pub(super) struct Dom {
    nodes: RefCell<Vec<Option<Node>>>,
    next: Cell<NodeId>,
    ops: RefCell<Vec<Op>>,
    warnings: Warnings,
}

impl Dom {
    pub(super) fn new() -> Self {
        Self {
            nodes: RefCell::new(vec![
                None,
                Some(Node {
                    text: false,
                    tag: None,
                    parent: DETACHED,
                    children: Vec::new(),
                    listening: Vec::new(),
                }),
            ]),
            next: Cell::new(2),
            ops: RefCell::new(Vec::new()),
            warnings: Warnings::default(),
        }
    }

    pub(super) fn take(&self, out: &mut Vec<Op>) {
        out.append(&mut self.ops.borrow_mut());
    }

    fn push(&self, op: Op) {
        self.ops.borrow_mut().push(op);
    }

    fn id_of(value: &Value) -> Result<NodeId, PropError> {
        value
            .node()
            .ok_or_else(|| PropError("nlayer: not a node".into()))
    }

    fn create(&self, tag: Option<Tag>, text: Option<String>) -> Result<Value, PropError> {
        let id = self.next.get();
        let next = id
            .checked_add(2)
            .ok_or_else(|| PropError("node ids exhausted".into()))?;
        self.next.set(next);
        let is_text = text.is_some();
        {
            let mut nodes = self.nodes.borrow_mut();
            if nodes.len() <= id as usize {
                nodes.resize_with(id as usize + 1, || None);
            }
            nodes[id as usize] = Some(Node {
                text: is_text,
                tag,
                parent: DETACHED,
                children: Vec::new(),
                listening: Vec::new(),
            });
        }
        match text {
            Some(text) => self.push(Op::CreateText { id, text }),
            None => self.push(Op::Create {
                id,
                tag: tag.unwrap_or(Tag::View),
            }),
        }
        Value::node_object(id, is_text)
    }

    fn tag_of(&self, id: NodeId) -> Option<Tag> {
        self.nodes.borrow().get(id as usize)?.as_ref()?.tag
    }

    fn is_text(&self, id: NodeId) -> bool {
        self.nodes
            .borrow()
            .get(id as usize)
            .and_then(|n| n.as_ref())
            .is_some_and(|n| n.text)
    }

    /// A new reference to a mounted node, taken without holding `nodes`
    /// across the page call.
    fn share_child(&self, id: NodeId) -> Option<Value> {
        let raw = {
            let nodes = self.nodes.borrow();
            let parent = nodes.get(id as usize)?.as_ref()?.parent;
            if parent == DETACHED {
                return None;
            }
            nodes
                .get(parent as usize)?
                .as_ref()?
                .children
                .iter()
                .find(|child| child.id == id)
                .map(|child| child.value.0)
        };
        raw.map(Value::share)
    }

    /// Pulls `id` out of its parent without emitting an op. `None` when it
    /// is already detached.
    fn detach_value(&self, id: NodeId) -> Option<Value> {
        let mut nodes = self.nodes.borrow_mut();
        let parent = nodes.get(id as usize)?.as_ref()?.parent;
        if parent == DETACHED {
            return None;
        }
        let children = &mut nodes.get_mut(parent as usize)?.as_mut()?.children;
        let index = children.iter().position(|child| child.id == id)?;
        let child = children.remove(index);
        if let Some(node) = nodes.get_mut(id as usize).and_then(|slot| slot.as_mut()) {
            node.parent = DETACHED;
        }
        Some(child.value)
    }

    fn insert(
        &self,
        parent: NodeId,
        child: Value,
        before: Option<NodeId>,
    ) -> Result<(), PropError> {
        let child_id = Self::id_of(&child)?;
        if child_id == ROOT_ID {
            return Err(PropError("cannot insert the root".into()));
        }
        if before == Some(child_id) {
            return Ok(());
        }
        let mut ancestor = parent;
        while ancestor != DETACHED {
            if ancestor == child_id {
                return Err(PropError(
                    "nlayer: a node cannot be inserted into itself".into(),
                ));
            }
            ancestor = self
                .nodes
                .borrow()
                .get(ancestor as usize)
                .and_then(|n| n.as_ref())
                .map(|n| n.parent)
                .ok_or_else(|| PropError("released parent node".into()))?;
        }
        let old_parent = {
            let nodes = self.nodes.borrow();
            nodes
                .get(child_id as usize)
                .and_then(|n| n.as_ref())
                .ok_or_else(|| PropError("released child node".into()))?
                .parent
        };
        if old_parent == parent {
            let next = self.next_sibling(child_id);
            let in_place = match (next, before) {
                (None, None) => true,
                (Some(n), Some(b)) => n == b,
                _ => false,
            };
            if in_place {
                return Ok(());
            }
        }
        if self.is_text(child_id) && self.tag_of(parent) != Some(Tag::Text) {
            self.warnings.warn(format!(
                "a string in {} is not shown: text shows in a <Text>",
                match self.tag_of(parent) {
                    None => "the root".to_owned(),
                    Some(tag) => format!("<{tag:?}>"),
                }
            ));
        }
        let before_id = {
            let nodes = self.nodes.borrow();
            let children = &nodes
                .get(parent as usize)
                .and_then(|n| n.as_ref())
                .ok_or_else(|| PropError("released parent node".into()))?
                .children;
            before.filter(|id| children.iter().any(|child| child.id == *id))
        };
        let stored = self.detach_value(child_id).unwrap_or(child);
        {
            let mut nodes = self.nodes.borrow_mut();
            let parent_node = nodes
                .get_mut(parent as usize)
                .and_then(|n| n.as_mut())
                .ok_or_else(|| PropError("released parent node".into()))?;
            let pos = match before_id {
                Some(before) => parent_node
                    .children
                    .iter()
                    .position(|child| child.id == before)
                    .unwrap_or(parent_node.children.len()),
                None => parent_node.children.len(),
            };
            parent_node.children.insert(
                pos,
                Child {
                    id: child_id,
                    value: stored,
                },
            );
        }
        self.nodes.borrow_mut()[child_id as usize]
            .as_mut()
            .ok_or_else(|| PropError("released child node".into()))?
            .parent = parent;
        self.push(Op::Insert {
            parent,
            child: child_id,
            before: before_id,
        });
        Ok(())
    }

    fn next_sibling(&self, child: NodeId) -> Option<NodeId> {
        let nodes = self.nodes.borrow();
        let parent = nodes.get(child as usize)?.as_ref()?.parent;
        if parent == DETACHED {
            return None;
        }
        let children = &nodes.get(parent as usize)?.as_ref()?.children;
        let index = children.iter().position(|item| item.id == child)?;
        children.get(index + 1).map(|item| item.id)
    }

    fn remove(&self, parent: NodeId, child: NodeId) {
        let value = {
            let mut nodes = self.nodes.borrow_mut();
            let attached = nodes
                .get(child as usize)
                .and_then(|n| n.as_ref())
                .is_some_and(|n| n.parent == parent);
            if !attached {
                return;
            }
            let Some(parent_node) = nodes.get_mut(parent as usize).and_then(|n| n.as_mut()) else {
                return;
            };
            let Some(index) = parent_node
                .children
                .iter()
                .position(|item| item.id == child)
            else {
                return;
            };
            let value = parent_node.children.remove(index).value;
            if let Some(child_node) = nodes.get_mut(child as usize).and_then(|n| n.as_mut()) {
                child_node.parent = DETACHED;
            }
            value
        };
        self.push(Op::Remove { parent, child });
        drop(value);
    }

    fn flatten(&self, value: Value, out: &mut Vec<Item>, depth: u32) -> Result<(), PropError> {
        if depth > MAX_DEPTH {
            return Err(PropError("nlayer: children nested too deeply".into()));
        }
        if value.node().is_some() {
            out.push(Item::Node(value));
            return Ok(());
        }
        match value.kind() {
            0 | 1 | 2 => Ok(()),
            3 | 4 => {
                out.push(Item::Text(value.to_string()?));
                Ok(())
            }
            5 => {
                let n = value.length().unwrap_or(0);
                for i in 0..n {
                    self.flatten(value.index(i)?, out, depth + 1)?;
                }
                Ok(())
            }
            6 => {
                let args = Value::array([])?;
                let returned = value.call(&Value::unit(), &args)?;
                self.flatten(returned, out, depth + 1)
            }
            _ => {
                self.warnings.warn(
                    "a child that is not an element, text, array or function is not rendered",
                );
                Ok(())
            }
        }
    }

    fn show(
        &self,
        parent: NodeId,
        value: &Value,
        old: &[NodeId],
        marker: Option<NodeId>,
    ) -> Result<Vec<Value>, PropError> {
        let mut items = Vec::new();
        self.flatten(value.clone(), &mut items, 0)?;
        let reused = match (old, items.first()) {
            ([only], Some(Item::Text(_))) if items.len() == 1 && self.is_text(*only) => Some(*only),
            _ => None,
        };
        let new = if let Some(id) = reused {
            let Item::Text(text) = items.pop().expect("the reused child is text") else {
                unreachable!("reused only when the only child is text");
            };
            self.push(Op::SetText { id, text });
            vec![
                self.share_child(id)
                    .ok_or_else(|| PropError("released text node".into()))?,
            ]
        } else {
            let mut nodes = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Item::Node(node) => nodes.push(node),
                    Item::Text(text) => nodes.push(self.create(None, Some(text))?),
                }
            }
            nodes
        };
        self.reconcile(parent, old, &new, marker)?;
        Ok(new)
    }

    fn reconcile(
        &self,
        parent: NodeId,
        old: &[NodeId],
        new: &[Value],
        marker: Option<NodeId>,
    ) -> Result<(), PropError> {
        let mut a = old.to_vec();
        let b_id = |index: usize| new[index].node().unwrap_or(DETACHED);
        let b_len = new.len();
        let (mut a_start, mut b_start) = (0usize, 0usize);
        let (mut a_end, mut b_end) = (a.len(), b_len);
        let after = match a.last() {
            Some(&last) => self.next_sibling(last).or(marker),
            None => marker,
        };
        let mut map: Option<FxHashMap<NodeId, usize>> = None;
        while a_start < a_end || b_start < b_end {
            if a_start < a_end && b_start < b_end && a[a_start] == b_id(b_start) {
                a_start += 1;
                b_start += 1;
                continue;
            }
            while a_end > a_start && b_end > b_start && a[a_end - 1] == b_id(b_end - 1) {
                a_end -= 1;
                b_end -= 1;
            }
            if a_end == a_start {
                let anchor = if b_end < b_len {
                    Some(b_id(b_end))
                } else {
                    after
                };
                while b_start < b_end {
                    self.insert(parent, new[b_start].clone(), anchor)?;
                    b_start += 1;
                }
            } else if b_end == b_start {
                while a_start < a_end {
                    if map.as_ref().is_none_or(|m| !m.contains_key(&a[a_start])) {
                        self.remove(parent, a[a_start]);
                    }
                    a_start += 1;
                }
            } else if a[a_start] == b_id(b_end - 1) && b_id(b_start) == a[a_end - 1] {
                a_end -= 1;
                let anchor = self.next_sibling(a[a_end]);
                let first_next = self.next_sibling(a[a_start]);
                self.insert(parent, new[b_start].clone(), first_next)?;
                b_start += 1;
                a_start += 1;
                b_end -= 1;
                self.insert(parent, new[b_end].clone(), anchor)?;
                a[a_end] = b_id(b_end);
            } else {
                let map =
                    map.get_or_insert_with(|| (b_start..b_end).map(|i| (b_id(i), i)).collect());
                match map.get(&a[a_start]).copied() {
                    Some(index) if b_start < index && index < b_end => {
                        let mut i = a_start;
                        let mut sequence = 1;
                        loop {
                            i += 1;
                            if !(i < a_end && i < b_end) {
                                break;
                            }
                            match map.get(&a[i]) {
                                Some(&t) if t == index + sequence => sequence += 1,
                                _ => break,
                            }
                        }
                        if sequence > index - b_start {
                            let anchor = a[a_start];
                            while b_start < index {
                                self.insert(parent, new[b_start].clone(), Some(anchor))?;
                                b_start += 1;
                            }
                        } else {
                            self.insert(parent, new[b_start].clone(), Some(a[a_start]))?;
                            if !map.contains_key(&a[a_start]) {
                                self.remove(parent, a[a_start]);
                            }
                            b_start += 1;
                            a_start += 1;
                        }
                    }
                    Some(_) => a_start += 1,
                    None => {
                        self.remove(parent, a[a_start]);
                        a_start += 1;
                    }
                }
            }
        }
        Ok(())
    }

    fn ids(value: &Value) -> Result<Vec<NodeId>, PropError> {
        if value.kind() != 5 {
            return Ok(Vec::new());
        }
        let n = value.length().unwrap_or(0);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(id) = value.index(i)?.node() {
                out.push(id);
            }
        }
        Ok(out)
    }

    pub(super) fn call(&self, op: u32, args: &Args) -> Result<Value, PropError> {
        match op {
            300 => {
                let name = args.index(0)?.as_string().unwrap_or_default();
                let tag = Tag::from_name(&name)
                    .ok_or_else(|| PropError(format!("unknown tag `{name}`")))?;
                self.create(Some(tag), None)
            }
            301 => {
                let text = args.index(0)?.to_string()?;
                self.create(None, Some(text))
            }
            302 => {
                let parent = Self::id_of(&args.index(0)?)?;
                let child = args.index(1)?;
                let before = args.index(2)?.node();
                self.insert(parent, child, before)?;
                Ok(Value::unit())
            }
            303 => {
                let id = Self::id_of(&args.index(0)?)?;
                let name = args.index(1)?.as_string().unwrap_or_default();
                let value = args.index(2)?;
                if let Some(op) =
                    protocol::decode_prop(id, self.tag_of(id), &name, &value, &self.warnings)?
                {
                    self.push(op);
                }
                Ok(Value::unit())
            }
            305 => {
                let id = Self::id_of(&args.index(0)?)?;
                let name = args.index(1)?.as_string().unwrap_or_default();
                let enabled = args.index(2)?.truthy();
                let Some(event) = EventKind::from_prop(&name) else {
                    self.warnings.warn(format!("unsupported event `{name}`"));
                    return Ok(Value::unit());
                };
                let changed = {
                    let mut nodes = self.nodes.borrow_mut();
                    let node = nodes
                        .get_mut(id as usize)
                        .and_then(|n| n.as_mut())
                        .ok_or_else(|| PropError("released node".into()))?;
                    let has = node.listening.contains(&event);
                    if enabled && !has {
                        node.listening.push(event);
                        true
                    } else if !enabled && has {
                        node.listening.retain(|kind| *kind != event);
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    self.push(Op::Listen { id, event, enabled });
                }
                Ok(Value::unit())
            }
            306 => {
                let parent = Self::id_of(&args.index(0)?)?;
                let child = Self::id_of(&args.index(1)?)?;
                self.remove(parent, child);
                Ok(Value::unit())
            }
            307 => {
                let id = args.index(0)?.number_value().unwrap_or(0.0) as NodeId;
                if id <= ROOT_ID {
                    return Ok(Value::unit());
                }
                let Some(node) = self
                    .nodes
                    .borrow_mut()
                    .get_mut(id as usize)
                    .and_then(Option::take)
                else {
                    return Ok(Value::unit());
                };
                let parent = node.parent;
                for child in &node.children {
                    if let Some(Some(child_node)) =
                        self.nodes.borrow_mut().get_mut(child.id as usize)
                    {
                        child_node.parent = DETACHED;
                    }
                }
                let removed = if parent == DETACHED {
                    Vec::new()
                } else {
                    let mut nodes = self.nodes.borrow_mut();
                    if let Some(Some(parent_node)) = nodes.get_mut(parent as usize) {
                        let mut kept = Vec::new();
                        let mut removed = Vec::new();
                        for child in parent_node.children.drain(..) {
                            if child.id == id {
                                removed.push(child);
                            } else {
                                kept.push(child);
                            }
                        }
                        parent_node.children = kept;
                        removed
                    } else {
                        Vec::new()
                    }
                };
                self.push(Op::Release { id });
                drop(removed);
                drop(node);
                Ok(Value::unit())
            }
            308 => {
                let parent = Self::id_of(&args.index(0)?)?;
                let value = args.index(1)?;
                let old = Self::ids(&args.index(2)?)?;
                let before = args.index(3)?.node();
                let shown = self.show(parent, &value, &old, before)?;
                Value::array(shown)
            }
            _ => Err(PropError("unknown dom operation".into())),
        }
    }
}

impl Runtime {
    pub(super) fn dom_call(&self, op: u32, args: &Args) -> Result<Value, PropError> {
        self.dom.call(op, args)
    }
}
