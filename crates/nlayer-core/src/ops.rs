//! Operations the JS renderer sends the shadow tree.
//!
//! `ResizeMode` is copied from `nsh_core::ops`. The `PropEnum` derive arrives
//! with the element macros. `Tag`, `EventKind` and `Op` are the view-tree
//! subset: text, image, scroll and input ops follow with those hosts.

use crate::{
    geometry::{Bounds, Point, Points, Size},
    style::{ObjectPosition, Style, StylePatch},
};

/// Identifier of a node, assigned by the JS renderer.
///
/// Ids are small, dense and increasing, so the core indexes them directly.
pub type NodeId = u32;

/// Id of the surface root view. It exists before any JS runs.
pub const ROOT_ID: NodeId = 1;

/// Element tag of a host node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// A container view (`<view>`).
    View,
}

impl Tag {
    /// Parses a host element's name (what `nlayer:rt`'s `element` takes).
    #[inline]
    pub fn from_name(name: &str) -> Option<Tag> {
        match name {
            "view" => Some(Tag::View),
            _ => None,
        }
    }
}

/// An event a node can listen to.
///
/// Discriminant order is the bit in [`EventMask`]. Append new kinds; do not
/// reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventKind {
    /// A tap / click.
    Press,
}

impl EventKind {
    /// Parses an event's protocol name such as `onPress`.
    #[inline]
    pub fn from_prop(name: &str) -> Option<EventKind> {
        match name {
            "onPress" => Some(EventKind::Press),
            _ => None,
        }
    }

    /// The protocol name of this event.
    #[inline]
    pub const fn prop_name(self) -> &'static str {
        match self {
            EventKind::Press => "onPress",
        }
    }

    /// The handler prop apps set for this event.
    #[inline]
    pub const fn handler_name(self) -> &'static str {
        self.prop_name()
    }

    /// Parses a handler prop name such as `onPress`.
    #[inline]
    pub fn from_handler(name: &str) -> Option<EventKind> {
        Self::from_prop(name)
    }
}

/// A set of event kinds: one bit per kind, room for 128.
///
/// Two words, not a `u128`: views keep it in Objective-C ivars, which take
/// no 16-byte alignment. Copied from `nsh_core::ops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventMask([u64; 2]);

impl std::ops::BitOr for EventMask {
    type Output = EventMask;

    #[inline]
    fn bitor(self, other: EventMask) -> EventMask {
        EventMask([self.0[0] | other.0[0], self.0[1] | other.0[1]])
    }
}

impl EventMask {
    /// Returns true if `event` is in the set.
    #[inline]
    pub const fn contains(self, event: EventKind) -> bool {
        let k = event as usize;
        self.0[k / 64] & (1 << (k % 64)) != 0
    }

    /// The raw bit set, indexed by `EventKind as u8`.
    #[inline]
    pub const fn bits(self) -> u128 {
        self.0[0] as u128 | (self.0[1] as u128) << 64
    }

    /// Adds or removes `event`.
    #[inline]
    pub fn set(&mut self, event: EventKind, enabled: bool) {
        let k = event as usize;
        if enabled {
            self.0[k / 64] |= 1 << (k % 64);
        } else {
            self.0[k / 64] &= !(1 << (k % 64));
        }
    }

    /// Returns true if no event is set.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }
}

/// How an image fits its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeMode {
    /// Fill the box, cropping (default).
    #[default]
    Cover,
    /// Fit inside the box, letterboxing.
    Contain,
    /// Stretch to the box.
    Stretch,
    /// Center at natural size, cropped where larger than the box (`object-fit: none`).
    Center,
    /// Center at natural size, shrunk to fit the box where larger (`object-fit: scale-down`).
    /// JS name: `scale-down`.
    ScaleDown,
}

impl ResizeMode {
    /// Where content of `natural` size goes in a box of `size`.
    pub fn fit(
        self,
        natural: Size<Points>,
        size: Size<Points>,
        position: ObjectPosition,
    ) -> Bounds<Points> {
        let (nw, nh) = (natural.width.0, natural.height.0);
        if nw <= 0.0 || nh <= 0.0 || self == ResizeMode::Stretch {
            return Bounds {
                origin: Point::default(),
                size,
            };
        }
        let contain = (size.width.0 / nw).min(size.height.0 / nh);
        let scale = match self {
            ResizeMode::Cover => (size.width.0 / nw).max(size.height.0 / nh),
            ResizeMode::Contain => contain,
            ResizeMode::Center => 1.0,
            ResizeMode::ScaleDown => contain.min(1.0),
            ResizeMode::Stretch => 1.0,
        };
        let content = Size {
            width: Points(nw * scale),
            height: Points(nh * scale),
        };
        Bounds {
            origin: position.place(size, content),
            size: content,
        }
    }
}

/// A mutation produced by the JS renderer.
#[derive(Debug)]
pub enum Op {
    /// Creates a detached element node.
    Create {
        /// New node id.
        id: NodeId,
        /// Element tag.
        tag: Tag,
    },
    /// Replaces the whole style of a node.
    SetStyle {
        /// Node id.
        id: NodeId,
        /// The new style; boxed because layout styles are large.
        style: Box<Style>,
        /// The declarations `style` was built from, applied again over the
        /// node's class styles. v1 has no class table, so the shadow tree
        /// applies `style` itself.
        patch: StylePatch,
    },
    /// Enables or disables an event listener on a node.
    Listen {
        /// Node id.
        id: NodeId,
        /// Event kind.
        event: EventKind,
        /// Whether a handler is attached.
        enabled: bool,
    },
    /// Inserts (or moves) `child` into `parent` before `before`, or appends.
    Insert {
        /// Parent node id.
        parent: NodeId,
        /// Child node id.
        child: NodeId,
        /// Sibling to insert before; `None` appends.
        before: Option<NodeId>,
    },
    /// Detaches `child` from `parent`. The child stays alive until released.
    Remove {
        /// Parent node id.
        parent: NodeId,
        /// Child node id.
        child: NodeId,
    },
    /// The JS node was garbage collected: free the node and its native view.
    Release {
        /// Node id.
        id: NodeId,
    },
}
