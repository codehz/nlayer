//! Shadow tree.
//!
//! Apply, commit and frame emission for views and images, extracted from
//! `nsh_core::core` and cut to those hosts. Text, scroll and input follow
//! in this module; they are not stubbed here.

mod layout_tree;

use std::mem;

use taffy::{AvailableSpace, Layout, Position, Style as TaffyStyle};

use layout_tree::LayoutTree;

use crate::{
    geometry::{Bounds, Points, Size, rect},
    mount::{MountBatch, MountItem, ViewKind},
    ops::{EventMask, ImageSource, NodeId, Op, Prop, ROOT_ID, ResizeMode, Tag},
    style::{self, Style, ViewStyle},
};

/// Slab index of a node. Internal links use keys, never ids.
type Key = u32;

const NO_KEY: Key = Key::MAX;

type LayoutId = taffy::NodeId;

mod dirty {
    /// Emit [`crate::MountItem::ViewStyle`].
    pub(super) const VIEW: u32 = 1 << 0;
    /// Emit [`crate::MountItem::Events`].
    pub(super) const EVENTS: u32 = 1 << 1;
    /// Emit [`crate::MountItem::Image`].
    pub(super) const IMAGE: u32 = 1 << 2;
}

/// Host kind. Text, scroll and input join this when their paths are copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    View,
    Image,
}

impl Kind {
    fn of(tag: Tag) -> Self {
        match tag {
            Tag::View => Kind::View,
            Tag::Image => Kind::Image,
        }
    }

    fn view_kind(self) -> ViewKind {
        match self {
            Kind::View => ViewKind::View,
            Kind::Image => ViewKind::Image,
        }
    }
}

/// An op referenced a node in an impossible way. This is a renderer bug;
/// the op is skipped and the tree stays consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// The id is unknown (never created or already released).
    UnknownNode(NodeId),
    /// The id is already in use.
    DuplicateNode(NodeId),
    /// The child is not a child of the given parent.
    NotAChild(NodeId),
    /// The `before` sibling is not a child of the given parent.
    BadAnchor(NodeId),
    /// The insert would create a cycle.
    Cycle(NodeId),
    /// Creating the node would exceed the node quota ([`Core::set_max_nodes`]).
    TooManyNodes(usize),
    /// A new id far beyond the ids in use (ids index a table, so they must
    /// stay dense).
    IdOutOfRange(NodeId),
}

impl ApplyError {
    /// The app went over a quota or sent what no runtime sends: it should
    /// be stopped rather than continue.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            ApplyError::TooManyNodes(_) | ApplyError::IdOutOfRange(_)
        )
    }
}

struct Node {
    id: NodeId,
    kind: Kind,
    parent: Key,
    children: Vec<Key>,
    /// Layout node. Present for every view.
    layout: LayoutId,
    view_style: ViewStyle,
    /// Start / end properties, if any, so a direction change can place them
    /// again.
    logical: Option<Box<style::Logical>>,
    events: EventMask,
    /// `objectFit`. `None` is the image default (cover).
    object_fit: Option<ResizeMode>,
    /// Present exactly for [`Kind::Image`]: `None` until `src` is set.
    image: Option<Option<ImageSource>>,
    /// `zIndex`: native views are ordered by it (stable) among siblings.
    z_index: i32,
    /// Laid out right to left (the resolved `direction`).
    rtl: bool,
    /// `direction`: `None` inherits.
    direction: Option<bool>,
    /// The native view exists.
    created: bool,
    /// Last frame sent to the platform.
    frame: Bounds<Points>,
    dirty: u32,
    queued: bool,
}

impl Node {
    fn new(id: NodeId, kind: Kind, layout: LayoutId) -> Self {
        Self {
            id,
            kind,
            parent: NO_KEY,
            children: Vec::new(),
            layout,
            view_style: ViewStyle::default(),
            logical: None,
            events: EventMask::default(),
            object_fit: None,
            image: matches!(kind, Kind::Image).then_some(None),
            z_index: 0,
            rtl: false,
            direction: None,
            created: false,
            frame: Bounds::default(),
            dirty: 0,
            queued: false,
        }
    }
}

/// The shadow tree of one surface.
///
/// Feed it [`Op`]s with [`Core::apply`], then call [`Core::commit`] once per
/// JS turn to get the [`MountBatch`] for the platform.
pub struct Core {
    nodes: slab::Slab<Node>,
    /// `slots[id] = key + 1`, 0 if free. Ids index the table directly.
    slots: Vec<Key>,
    layout: LayoutTree<Option<Size<Points>>>,
    max_nodes: usize,
    root: Key,
    viewport: Size<Points>,
    mount: Vec<MountItem>,
    queue: Vec<Key>,
    needs_layout: bool,
    /// Device pixels per point. Bundled image variants are picked from it.
    pixel_ratio: f32,
}

impl Core {
    /// Creates a core whose root view (id [`ROOT_ID`]) fills `viewport`.
    /// The platform owns that view; it is not in the mount batch.
    pub fn new(viewport: Size<Points>) -> Self {
        let mut layout = LayoutTree::with_capacity(256);
        let root_layout = layout.new_leaf(root_style(viewport));
        let mut nodes = slab::Slab::with_capacity(256);
        let mut root = Node::new(ROOT_ID, Kind::View, root_layout);
        root.created = true;
        let root = nodes.insert(root) as Key;
        layout.set_tag(root_layout, root);
        let mut slots = vec![0; 256];
        slots[ROOT_ID as usize] = root + 1;
        Self {
            nodes,
            slots,
            layout,
            max_nodes: usize::MAX,
            root,
            viewport,
            mount: Vec::new(),
            queue: Vec::new(),
            needs_layout: true,
            pixel_ratio: 1.0,
        }
    }

    /// Caps how many nodes [`Op::Create`] may add.
    pub fn set_max_nodes(&mut self, max: usize) {
        self.max_nodes = max;
    }

    /// Changes the size of the root view.
    pub fn set_viewport(&mut self, viewport: Size<Points>) {
        if viewport != self.viewport {
            self.viewport = viewport;
            let root = self.nodes[self.root as usize].layout;
            self.layout.set_style(root, root_style(viewport));
            self.needs_layout = true;
        }
    }

    /// Device pixels per point. Images whose bundled variant changes are
    /// mounted again; their layout does not change.
    pub fn set_pixel_ratio(&mut self, pixel_ratio: f32) {
        if pixel_ratio == self.pixel_ratio {
            return;
        }
        let old = self.pixel_ratio;
        self.pixel_ratio = pixel_ratio;
        self.repick_images(old, pixel_ratio);
    }

    /// Applies one op from JS.
    pub fn apply(&mut self, op: Op) -> Result<(), ApplyError> {
        match op {
            Op::Create { id, tag } => self.create(id, tag),
            Op::SetStyle { id, style, .. } => self.set_style(id, *style),
            Op::SetProp { id, prop } => self.set_prop(id, prop),
            Op::Listen { id, event, enabled } => {
                let key = self.key(id)?;
                let node = &mut self.nodes[key as usize];
                let before = node.events;
                node.events.set(event, enabled);
                if node.events != before {
                    self.mark(key, dirty::EVENTS);
                }
                Ok(())
            }
            Op::Insert {
                parent,
                child,
                before,
            } => self.insert(parent, child, before),
            Op::Remove { parent, child } => {
                let parent = self.key(parent)?;
                let child = self.key(child)?;
                if self.nodes[child as usize].parent != parent {
                    return Err(ApplyError::NotAChild(self.nodes[child as usize].id));
                }
                self.unlink(child, true);
                Ok(())
            }
            Op::Release { id } => self.release(id),
        }
    }

    /// Finishes a turn: runs layout and returns the mount items, or `None`
    /// when nothing changed.
    pub fn commit(&mut self) -> Option<MountBatch> {
        self.flush_dirty();
        if mem::take(&mut self.needs_layout) {
            let relaid = self.relayout_boundaries();
            if self.layout_main() {
                let root = self.root;
                let children = mem::take(&mut self.nodes[root as usize].children);
                for &child in &children {
                    self.emit_frames(child);
                }
                self.nodes[root as usize].children = children;
            }
            for key in relaid {
                self.emit_frames(key);
            }
        }
        if self.mount.is_empty() {
            None
        } else {
            Some(MountBatch {
                items: mem::take(&mut self.mount),
            })
        }
    }

    fn key(&self, id: NodeId) -> Result<Key, ApplyError> {
        match self.slots.get(id as usize) {
            Some(&k) if k != 0 => Ok(k - 1),
            _ => Err(ApplyError::UnknownNode(id)),
        }
    }

    fn alloc(&mut self, id: NodeId, kind: Kind, layout: LayoutId) -> Result<Key, ApplyError> {
        let idx = id as usize;
        if self.nodes.len() >= self.max_nodes {
            return Err(ApplyError::TooManyNodes(self.max_nodes));
        }
        if idx > self.slots.len().saturating_add(self.max_nodes) {
            return Err(ApplyError::IdOutOfRange(id));
        }
        if idx >= self.slots.len() {
            self.slots.resize((idx + 1).next_power_of_two(), 0);
        } else if self.slots[idx] != 0 {
            return Err(ApplyError::DuplicateNode(id));
        }
        let key = self.nodes.insert(Node::new(id, kind, layout)) as Key;
        self.slots[idx] = key + 1;
        Ok(key)
    }

    fn create(&mut self, id: NodeId, tag: Tag) -> Result<(), ApplyError> {
        let kind = Kind::of(tag);
        let mut layout_style = Style::default().layout;
        style::resolve_normal_alignment(&mut layout_style);
        let layout = match kind {
            Kind::Image => self.layout.new_leaf_with_context(layout_style, None),
            Kind::View => self.layout.new_leaf(layout_style),
        };
        let key = self.alloc(id, kind, layout)?;
        self.layout.set_tag(layout, key);
        Ok(())
    }

    fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), ApplyError> {
        let fit = style.object_fit();
        let key = self.key(id)?;
        let Style {
            mut layout,
            mut view,
            logical,
            z_index,
            direction,
            ..
        } = style;
        let node = &mut self.nodes[key as usize];
        if key != self.root {
            node.direction = direction;
        }
        node.logical = (!logical.is_empty()).then(|| Box::new(logical.clone()));
        let rtl = self.resolved_rtl(key);
        style::resolve_normal_alignment(&mut layout);
        style::apply_direction(&mut layout, &mut view, &logical, rtl);
        let node = &mut self.nodes[key as usize];
        node.rtl = rtl;
        let view_changed = node.view_style != view;
        let z_changed = node.z_index != z_index;
        node.view_style = view;
        node.z_index = z_index;
        let fit_changed = node.image.is_some() && node.object_fit != fit;
        if fit_changed {
            node.object_fit = fit;
        }
        // `overflow: "hidden"` clips in layout as well as in paint. `Clip`,
        // not `Hidden`: both stop the overflow from propagating, but `Hidden`
        // is a scroll container in CSS and would also make the box's
        // automatic minimum size as a flex or grid item 0.
        let clip = if node.view_style.overflow_hidden {
            taffy::Overflow::Clip
        } else {
            taffy::Overflow::Visible
        };
        layout.overflow = taffy::Point { x: clip, y: clip };
        let l = node.layout;
        self.layout.set_style(l, layout);
        self.needs_layout = true;
        if z_changed {
            self.restack(key);
        }
        if view_changed {
            self.mark(key, dirty::VIEW);
        }
        if fit_changed {
            self.mark(key, dirty::IMAGE);
        }
        Ok(())
    }

    fn set_prop(&mut self, id: NodeId, prop: Prop) -> Result<(), ApplyError> {
        let key = self.key(id)?;
        match prop {
            Prop::Source(source) => {
                let Some(image) = self.nodes[key as usize].image.as_mut() else {
                    return Ok(());
                };
                let size = source.as_ref().and_then(|s| s.size);
                *image = source;
                self.mark(key, dirty::IMAGE);
                let layout = self.nodes[key as usize].layout;
                if let Some(natural) = self.layout.get_node_context_mut(layout)
                    && *natural != size
                {
                    *natural = size;
                    self.layout.mark_dirty(layout);
                    self.needs_layout = true;
                }
            }
        }
        Ok(())
    }

    /// The pixel ratio went from `old` to `new`: the images whose variant
    /// changes are sent again.
    fn repick_images(&mut self, old: f32, new: f32) {
        let images: Vec<Key> = self
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.image.as_ref().is_some_and(|source| {
                    source
                        .as_ref()
                        .is_some_and(|s| !s.scales.is_empty() && s.variant(old) != s.variant(new))
                })
            })
            .map(|(k, _)| k as Key)
            .collect();
        for key in images {
            self.mark(key, dirty::IMAGE);
        }
    }

    /// The direction `key` lays out in: its own, else its parent's (the
    /// root's while detached).
    fn resolved_rtl(&self, key: Key) -> bool {
        let node = &self.nodes[key as usize];
        if let Some(rtl) = node.direction {
            return rtl;
        }
        match node.parent {
            NO_KEY => key != self.root && self.nodes[self.root as usize].rtl,
            parent => self.nodes[parent as usize].rtl,
        }
    }

    fn insert(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        before: Option<NodeId>,
    ) -> Result<(), ApplyError> {
        let parent = self.key(parent_id)?;
        let child = self.key(child_id)?;
        let before = before.map(|b| self.key(b)).transpose()?;
        if child == self.root || self.is_ancestor(child, parent) {
            return Err(ApplyError::Cycle(child_id));
        }
        if let Some(b) = before
            && (b == child || self.nodes[b as usize].parent != parent)
        {
            return Err(ApplyError::BadAnchor(self.nodes[b as usize].id));
        }

        let parent_created = self.nodes[parent as usize].created;
        if self.nodes[child as usize].parent != NO_KEY {
            self.unlink(child, !parent_created);
        }

        let pos = match before {
            Some(b) => self.nodes[parent as usize]
                .children
                .iter()
                .position(|&c| c == b)
                .expect("anchor is a child"),
            None => self.nodes[parent as usize].children.len(),
        };
        self.nodes[child as usize].parent = parent;
        self.nodes[parent as usize].children.insert(pos, child);

        let index = pos;
        let parent_layout = self.nodes[parent as usize].layout;
        let child_layout = self.nodes[child as usize].layout;
        self.layout
            .insert_child_at_index(parent_layout, index as usize, child_layout);
        self.needs_layout = true;
        if parent_created {
            self.ensure_created(child);
            let before = self.native_before(parent, child);
            self.mount.push(MountItem::Insert {
                parent: self.nodes[parent as usize].id,
                child: self.nodes[child as usize].id,
                before,
            });
        }
        Ok(())
    }

    fn unlink(&mut self, key: Key, emit_remove: bool) {
        let parent = self.nodes[key as usize].parent;
        let siblings = &mut self.nodes[parent as usize].children;
        let pos = siblings
            .iter()
            .position(|&c| c == key)
            .expect("child of parent");
        siblings.remove(pos);
        self.nodes[key as usize].parent = NO_KEY;
        let (layout, created, id) = (
            self.nodes[key as usize].layout,
            self.nodes[key as usize].created,
            self.nodes[key as usize].id,
        );
        let parent_layout = self.nodes[parent as usize].layout;
        self.layout.remove_child(parent_layout, layout);
        self.needs_layout = true;
        if emit_remove && created && self.nodes[parent as usize].created {
            self.mount.push(MountItem::Remove { id });
        }
    }

    fn release(&mut self, id: NodeId) -> Result<(), ApplyError> {
        let key = self.key(id)?;
        if key == self.root {
            return Ok(());
        }
        if self.nodes[key as usize].parent != NO_KEY {
            self.unlink(key, false);
        }
        let node = self.nodes.remove(key as usize);
        self.slots[id as usize] = 0;
        for &c in &node.children {
            if let Some(child) = self.nodes.get_mut(c as usize) {
                child.parent = NO_KEY;
            }
        }
        self.layout.remove(node.layout);
        if node.created {
            self.mount.push(MountItem::Delete { id });
        }
        Ok(())
    }

    fn is_ancestor(&self, ancestor: Key, mut key: Key) -> bool {
        while key != NO_KEY {
            if key == ancestor {
                return true;
            }
            key = self.nodes[key as usize].parent;
        }
        false
    }

    /// The native sibling `child` goes below: native views are stacked by
    /// `zIndex`, then in tree order.
    fn native_before(&self, parent: Key, child: Key) -> Option<NodeId> {
        let children = &self.nodes[parent as usize].children;
        let z = self.nodes[child as usize].z_index;
        let pos = children.iter().position(|&c| c == child)?;
        let mut best: Option<(i32, usize, NodeId)> = None;
        for (i, &c) in children.iter().enumerate() {
            let n = &self.nodes[c as usize];
            if c == child || !n.created {
                continue;
            }
            if (n.z_index, i) > (z, pos) && best.is_none_or(|b| (n.z_index, i) < (b.0, b.1)) {
                best = Some((n.z_index, i, n.id));
            }
        }
        best.map(|b| b.2)
    }

    /// `key`'s `zIndex` changed: moves its native view among its siblings.
    fn restack(&mut self, key: Key) {
        let node = &self.nodes[key as usize];
        let (parent, id) = (node.parent, node.id);
        if parent == NO_KEY || !node.created {
            return;
        }
        if !self.nodes[parent as usize].created {
            return;
        }
        let before = self.native_before(parent, key);
        self.mount.push(MountItem::Insert {
            parent: self.nodes[parent as usize].id,
            child: id,
            before,
        });
    }

    /// Creates the native views of `key`'s subtree if needed, wiring up the
    /// children in order.
    fn ensure_created(&mut self, key: Key) {
        let node = &mut self.nodes[key as usize];
        if node.created {
            return;
        }
        node.created = true;
        node.frame = Bounds::UNSET;
        let id = node.id;
        let mut flags = 0;
        if node.view_style != ViewStyle::default() {
            flags |= dirty::VIEW;
        }
        if !node.events.is_empty() {
            flags |= dirty::EVENTS;
        }
        if node.image.as_ref().is_some_and(|source| source.is_some()) {
            flags |= dirty::IMAGE;
        }
        let kind = node.kind.view_kind();
        self.mount.push(MountItem::Create { id, kind });
        self.mark(key, flags);
        let mut children = mem::take(&mut self.nodes[key as usize].children);
        let original = children.clone();
        if children
            .iter()
            .any(|&c| self.nodes[c as usize].z_index != 0)
        {
            children.sort_by_key(|&c| self.nodes[c as usize].z_index);
        }
        for &child in &children {
            self.ensure_created(child);
            let child_id = self.nodes[child as usize].id;
            self.mount.push(MountItem::Insert {
                parent: id,
                child: child_id,
                before: None,
            });
        }
        self.nodes[key as usize].children = original;
    }

    fn mark(&mut self, key: Key, flags: u32) {
        if flags == 0 {
            return;
        }
        let node = &mut self.nodes[key as usize];
        node.dirty |= flags;
        if !node.queued {
            node.queued = true;
            self.queue.push(key);
        }
    }

    fn flush_dirty(&mut self) {
        let queue = mem::take(&mut self.queue);
        for key in queue {
            let Some(node) = self.nodes.get_mut(key as usize) else {
                continue;
            };
            let flags = mem::take(&mut node.dirty);
            node.queued = false;
            if !node.created {
                continue;
            }
            let id = node.id;
            if flags & dirty::VIEW != 0 {
                let style = self.mounted_view_style(key);
                self.mount.push(MountItem::ViewStyle { id, style });
            }
            if flags & dirty::EVENTS != 0 {
                let events = self.nodes[key as usize].events;
                self.mount.push(MountItem::Events { id, events });
            }
            if flags & dirty::IMAGE != 0
                && let Some(image) = &self.nodes[key as usize].image
            {
                let resize_mode = self.nodes[key as usize].object_fit.unwrap_or_default();
                let ratio = self.pixel_ratio;
                self.mount.push(MountItem::Image {
                    id,
                    source: image.as_ref().map(|s| s.resolved(ratio)),
                    resize_mode,
                });
            }
        }
    }

    /// The view style as platforms take it: translations by a fraction of
    /// the size resolved with the current frame.
    fn mounted_view_style(&self, key: Key) -> Box<ViewStyle> {
        let node = &self.nodes[key as usize];
        let mut style = Box::new(node.view_style.clone());
        if style.transform.has_fractions() {
            let size = if node.frame.size.width.is_nan() {
                Size::default()
            } else {
                node.frame.size
            };
            style.transform = style.transform.resolve_fractions(size);
        }
        style
    }

    fn positioning_parent(&self, layout: LayoutId) -> Option<LayoutId> {
        let mut parent = self.layout.parent(layout)?;
        match self.layout.style(layout).position {
            Position::Absolute => {
                while !self.layout.style(parent).position.is_positioned() {
                    let Some(above) = self.layout.parent(parent) else {
                        break;
                    };
                    parent = above;
                }
            }
            Position::Fixed => {
                while let Some(above) = self.layout.parent(parent) {
                    parent = above;
                }
            }
            _ => {}
        }
        Some(parent)
    }

    fn layout_origin(&self, layout: LayoutId) -> taffy::Point<f32> {
        let mut above = Vec::new();
        let mut at = self.positioning_parent(layout);
        while let Some(node) = at {
            above.push(node);
            at = self.positioning_parent(node);
        }
        above
            .iter()
            .rev()
            .fold(taffy::Point::ZERO, |origin, &node| {
                let l = self.layout.unrounded_layout(node);
                taffy::Point {
                    x: origin.x + l.location.x,
                    y: origin.y + l.location.y,
                }
            })
    }

    fn layout_main(&mut self) -> bool {
        let root = self.nodes[self.root as usize].layout;
        let available = taffy::Size {
            width: AvailableSpace::Definite(self.viewport.width.0),
            height: AvailableSpace::Definite(self.viewport.height.0),
        };
        let main = self.layout.dirty(root);
        if main {
            self.layout
                .compute_layout_with_measure(root, available, measure_leaf);
        }
        main
    }

    fn relayout_boundaries(&mut self) -> Vec<Key> {
        self.layout.relayout_boundaries(measure_leaf)
    }

    fn displays_none(&self, key: Key) -> bool {
        let mut k = key;
        while k != NO_KEY {
            let layout = self.nodes[k as usize].layout;
            if self.layout.style(layout).display == taffy::Display::None {
                return true;
            }
            k = self.nodes[k as usize].parent;
        }
        false
    }

    fn emit_frames(&mut self, key: Key) {
        let parent = self.nodes[key as usize].parent;
        let hidden = parent != NO_KEY && self.displays_none(parent);
        let origin = self.layout.parent(self.nodes[key as usize].layout).map_or(
            taffy::Point::ZERO,
            |parent| {
                let origin = self.layout_origin(parent);
                let location = self.layout.unrounded_layout(parent).location;
                taffy::Point {
                    x: origin.x + location.x,
                    y: origin.y + location.y,
                }
            },
        );
        self.emit_frames_in(key, hidden, origin);
    }

    fn emit_frames_in(&mut self, key: Key, hidden: bool, origin: taffy::Point<f32>) {
        let layout = self.nodes[key as usize].layout;
        if !self.nodes[key as usize].created {
            return;
        }
        let hidden = hidden || self.layout.style(layout).display == taffy::Display::None;
        if hidden {
            self.set_frame(key, Bounds::default());
            let children = mem::take(&mut self.nodes[key as usize].children);
            for &child in &children {
                self.emit_frames_in(child, true, origin);
            }
            self.nodes[key as usize].children = children;
            return;
        }
        let unrounded = *self.layout.unrounded_layout(layout);
        let containing = if self.layout.style(layout).position.is_out_of_flow() {
            self.layout_origin(layout)
        } else {
            origin
        };
        let inner = taffy::Point {
            x: containing.x + unrounded.location.x,
            y: containing.y + unrounded.location.y,
        };
        let l = rounded(&unrounded, containing);
        let frame = rect(
            l.location.x + round(containing.x) - round(origin.x),
            l.location.y + round(containing.y) - round(origin.y),
            l.size.width,
            l.size.height,
        );
        self.set_frame(key, frame);
        let children = mem::take(&mut self.nodes[key as usize].children);
        for &child in &children {
            self.emit_frames_in(child, false, inner);
        }
        self.nodes[key as usize].children = children;
    }

    fn set_frame(&mut self, key: Key, frame: Bounds<Points>) {
        let node = &mut self.nodes[key as usize];
        if node.frame != frame {
            node.frame = frame;
            let id = node.id;
            self.mount.push(MountItem::Frame { id, frame });
            if node.view_style.transform.has_fractions() {
                let style = self.mounted_view_style(key);
                self.mount.push(MountItem::ViewStyle { id, style });
            }
        }
    }
}

fn measure_leaf(
    input: taffy::LayoutInput,
    _: taffy::NodeId,
    context: Option<&mut Option<Size<Points>>>,
    style: &TaffyStyle,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(
        input,
        style,
        |_, _| 0.0,
        |known, _| match context {
            Some(Some(natural)) => image_size(*natural, known.width, known.height),
            _ => taffy::Size::ZERO,
        },
    )
}

/// Size of an image with a known natural size: fill a known dimension and
/// keep the aspect ratio for the other.
fn image_size(natural: Size<Points>, w: Option<f32>, h: Option<f32>) -> taffy::Size<f32> {
    let ratio = if natural.height > Points::ZERO {
        natural.width / natural.height
    } else {
        1.0
    };
    let (width, height) = match (w, h) {
        (Some(w), None) => (w, w / ratio),
        (None, Some(h)) => (h * ratio, h),
        (Some(w), Some(h)) => (w, h),
        (None, None) => (natural.width.0, natural.height.0),
    };
    taffy::Size { width, height }
}

fn root_style(viewport: Size<Points>) -> TaffyStyle {
    let mut style = TaffyStyle {
        size: taffy::Size {
            width: taffy::Dimension::length(viewport.width.0),
            height: taffy::Dimension::length(viewport.height.0),
        },
        ..Style::default().layout
    };
    style::resolve_normal_alignment(&mut style);
    style
}

/// Taffy's rounding of a coordinate.
fn round(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// `layout` rounded as Taffy's rounding pass (`taffy::round_layout`) rounds
/// it, `origin` being the unrounded position of the node around it in its
/// layout root: edges are rounded where they are in the root, so adjacent
/// views stay adjacent. The core rounds itself: Taffy's pass reaches the
/// absolutely positioned nodes through their containing block's list,
/// which it can leave holding removed nodes (a panic) or never fill (a
/// hidden subtree's), while here every node is reached through its parent;
/// out-of-flow offsets use their Taffy containing block.
fn rounded(layout: &Layout, origin: taffy::Point<f32>) -> Layout {
    let mut out = *layout;
    let (x, y) = (origin.x + layout.location.x, origin.y + layout.location.y);
    out.location.x = round(x) - round(origin.x);
    out.location.y = round(y) - round(origin.y);
    out.size.width = round(x + layout.size.width) - round(x);
    out.size.height = round(y + layout.size.height) - round(y);
    out.scrollbar_size.width = round(layout.scrollbar_size.width);
    out.scrollbar_size.height = round(layout.scrollbar_size.height);
    out.border.left = round(x + layout.border.left) - round(x);
    out.border.right =
        round(x + layout.size.width) - round(x + layout.size.width - layout.border.right);
    out.border.top = round(y + layout.border.top) - round(y);
    out.border.bottom =
        round(y + layout.size.height) - round(y + layout.size.height - layout.border.bottom);
    out.padding.left = round(x + layout.padding.left) - round(x);
    out.padding.right =
        round(x + layout.size.width) - round(x + layout.size.width - layout.padding.right);
    out.padding.top = round(y + layout.padding.top) - round(y);
    out.padding.bottom =
        round(y + layout.size.height) - round(y + layout.size.height - layout.padding.bottom);
    let o = layout.scrollable_overflow_rect;
    out.scrollable_overflow_rect.left = round(x + o.left) - round(x);
    out.scrollable_overflow_rect.right = round(x + o.right) - round(x);
    out.scrollable_overflow_rect.top = round(y + o.top) - round(y);
    out.scrollable_overflow_rect.bottom = round(y + o.bottom) - round(y);
    out
}

#[cfg(test)]
mod tests {
    use taffy::{Dimension, Display, Position};

    use super::*;
    use crate::{
        geometry::{px, size, size_pt},
        ops::{EventKind, ImageScales, ImageSource, Prop, ResizeMode},
        style::{StylePatch, StyleValue},
    };

    fn core() -> Core {
        Core::new(size(px(100.0), px(200.0)))
    }

    fn sized(width: f32, height: f32) -> Style {
        let mut style = Style::default();
        style.layout.size = taffy::Size {
            width: Dimension::length(width),
            height: Dimension::length(height),
        };
        style
    }

    fn apply_style(core: &mut Core, id: NodeId, style: Style) {
        core.apply(Op::SetStyle {
            id,
            style: Box::new(style),
            patch: StylePatch::default(),
        })
        .unwrap();
    }

    fn frame(batch: &MountBatch, id: NodeId) -> Bounds<Points> {
        batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::Frame { id: i, frame } if *i == id => Some(*frame),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no frame for {id}"))
    }

    #[test]
    fn column_flex_stacks_children_and_rounds_in_the_root() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut top = sized(100.0, 20.0);
        top.layout.margin.left = taffy::LengthPercentageAuto::length(0.6);
        top.layout.size.width = Dimension::length(10.4);
        apply_style(&mut c, 2, top);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(100.0, 30.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 3,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let top = frame(&batch, 2);
        let bottom = frame(&batch, 3);
        assert_eq!(top.origin.x, px(1.0));
        assert_eq!(top.size.width, px(10.0));
        assert_eq!(top.origin.y, px(0.0));
        assert_eq!(top.size.height, px(20.0));
        assert_eq!(bottom.origin.y, px(20.0));
        assert_eq!(bottom.size.height, px(30.0));
        assert!(c.commit().is_none());
    }

    #[test]
    fn a_grid_places_cells_in_document_order() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut grid = sized(100.0, 80.0);
        grid.layout.display = Display::Grid;
        grid.layout.grid_template_columns = vec![
            taffy::style_helpers::length(40.0),
            taffy::style_helpers::length(60.0),
        ];
        grid.layout.grid_template_rows = vec![taffy::style_helpers::length(30.0)];
        apply_style(&mut c, 2, grid);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        for id in [3, 4] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(10.0, 10.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        let batch = c.commit().unwrap();
        assert_eq!(
            frame(&batch, 3).origin,
            crate::geometry::point(px(0.0), px(0.0))
        );
        assert_eq!(
            frame(&batch, 4).origin,
            crate::geometry::point(px(40.0), px(0.0))
        );
    }

    #[test]
    fn an_absolute_child_is_placed_in_its_containing_block() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut style = sized(10.0, 12.0);
        style.layout.position = Position::Absolute;
        style.layout.inset = taffy::Rect {
            top: taffy::style_helpers::length(4.0),
            left: taffy::style_helpers::length(6.0),
            right: taffy::style_helpers::auto(),
            bottom: taffy::style_helpers::auto(),
        };
        apply_style(&mut c, 2, style);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let frame = frame(&batch, 2);
        assert_eq!(frame.origin, crate::geometry::point(px(6.0), px(4.0)));
        assert_eq!(frame.size, size(px(10.0), px(12.0)));
    }

    #[test]
    fn display_none_clears_the_frame() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(40.0, 20.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let shown = c.commit().unwrap();
        assert_eq!(frame(&shown, 2).size, size(px(40.0), px(20.0)));
        let mut hidden = sized(40.0, 20.0);
        hidden.layout.display = Display::None;
        apply_style(&mut c, 2, hidden);
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2), Bounds::default());
    }

    #[test]
    fn on_press_is_mounted_with_the_view() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Listen {
            id: 2,
            event: EventKind::Press,
            enabled: true,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let events = batch.items.iter().find_map(|item| match item {
            MountItem::Events { id: 2, events } => Some(*events),
            _ => None,
        });
        assert_eq!(
            events,
            Some(
                EventMask::default() | {
                    let mut mask = EventMask::default();
                    mask.set(EventKind::Press, true);
                    mask
                }
            )
        );
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create {
                id: 2,
                kind: ViewKind::View
            }
        )));
    }

    #[test]
    fn a_higher_z_index_is_stacked_above() {
        let mut c = core();
        for id in [2, 3] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            c.apply(Op::Insert {
                parent: ROOT_ID,
                child: id,
                before: None,
            })
            .unwrap();
        }
        c.commit().unwrap();
        let mut high = Style::default();
        high.z_index = 5;
        apply_style(&mut c, 3, high);
        let mut mid = Style::default();
        mid.z_index = 1;
        apply_style(&mut c, 2, mid);
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: Some(3),
            }
        )));
    }

    #[test]
    fn remove_and_release_emit_in_order() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.commit().unwrap();
        c.apply(Op::Remove {
            parent: ROOT_ID,
            child: 2,
        })
        .unwrap();
        c.apply(Op::Release { id: 2 }).unwrap();
        let batch = c.commit().unwrap();
        let kinds: Vec<_> = batch
            .items
            .iter()
            .filter_map(|item| match item {
                MountItem::Remove { id: 2 } => Some("remove"),
                MountItem::Delete { id: 2 } => Some("delete"),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, ["remove", "delete"]);
        assert_eq!(
            c.apply(Op::SetStyle {
                id: 2,
                style: Box::new(Style::default()),
                patch: StylePatch::default(),
            }),
            Err(ApplyError::UnknownNode(2))
        );
    }

    #[test]
    fn insert_rejects_a_cycle() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        assert_eq!(
            c.apply(Op::Insert {
                parent: 3,
                child: 2,
                before: None,
            }),
            Err(ApplyError::Cycle(2))
        );
    }

    fn source(uri: &str, width: f32, height: f32, scales: &[f32]) -> ImageSource {
        ImageSource {
            uri: uri.into(),
            size: Some(size_pt(width, height)),
            scales: scales.iter().copied().collect(),
        }
    }

    #[test]
    fn an_image_uses_its_natural_size_and_aspect_ratio() {
        let mut c = Core::new(size(px(320.0), px(480.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::Image,
        })
        .unwrap();
        let mut narrow = Style::default();
        narrow.set("width", StyleValue::Number(50.0)).unwrap();
        narrow
            .set("alignSelf", StyleValue::Str("flex-start"))
            .unwrap();
        apply_style(&mut c, 3, narrow);
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 3,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create {
                id: 2,
                kind: ViewKind::Image
            }
        )));
        assert_eq!(frame(&batch, 2).size, size(px(320.0), px(160.0)));
        assert_eq!(frame(&batch, 3).size, size(px(50.0), px(25.0)));
    }

    #[test]
    fn object_fit_is_mounted_on_the_image() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source("https://example.com/a.png", 40.0, 30.0, &[]))),
        })
        .unwrap();
        let mut style = Style::default();
        style.set("objectFit", StyleValue::Str("contain")).unwrap();
        apply_style(&mut c, 2, style);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let fitted = batch.items.iter().rev().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                resize_mode,
                source,
            } => Some((*resize_mode, source.as_ref().map(|s| s.uri.clone()))),
            _ => None,
        });
        assert_eq!(
            fitted,
            Some((
                ResizeMode::Contain,
                Some(std::sync::Arc::from("https://example.com/a.png"))
            ))
        );
    }

    #[test]
    fn a_bundled_image_picks_the_screen_density() {
        let mut c = core();
        c.set_pixel_ratio(2.0);
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source(
                "asset:/logo.png",
                40.0,
                30.0,
                &[1.0, 2.0, 2.5],
            ))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let uri = batch.items.iter().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                source: Some(source),
                ..
            } => Some(source.uri.clone()),
            _ => None,
        });
        assert_eq!(uri.as_deref(), Some("asset:/logo@2x.png"));
        assert_eq!(
            source("asset:/logo.png", 1.0, 1.0, &[1.0, 2.0, 2.5]).scales,
            ImageScales::NONE.with(1.0).with(2.0)
        );
        c.set_pixel_ratio(1.0);
        let batch = c.commit().unwrap();
        let uri = batch.items.iter().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                source: Some(source),
                ..
            } => Some(source.uri.clone()),
            _ => None,
        });
        assert_eq!(uri.as_deref(), Some("asset:/logo.png"));
        c.set_pixel_ratio(1.0);
        assert!(c.commit().is_none());
    }
}
