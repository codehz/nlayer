//! Operations the JS renderer sends the shadow tree.
//!
//! `ResizeMode`, `ImageSource` and `ImageScales` are copied from
//! `nsh_core::ops`. The `PropEnum` derive arrives with the element macros.
//! Scroll and input ops follow with those hosts. `data:` image bytes are
//! not part of v1.

use std::sync::Arc;

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
    /// A text block, or a span when nested in another text (`<text>`).
    Text,
    /// An image (`<image>`).
    Image,
}
impl Tag {
    /// Parses a host element's name (what `nlayer:rt`'s `element` takes).
    #[inline]
    pub fn from_name(name: &str) -> Option<Tag> {
        match name {
            "view" => Some(Tag::View),
            "text" => Some(Tag::Text),
            "image" => Some(Tag::Image),
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

/// An image source.
///
/// Copied from `nsh_core::ops`. v1 sources are a bundled `asset:` URI or an
/// `http(s)` URL. Bytes carried in the URI (`data:`) are not decoded here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageSource {
    /// `https://…` or `asset:/path` (bundled file).
    pub uri: Arc<str>,
    /// Natural size in points, when known up front (bundled assets): used
    /// as the intrinsic layout size.
    pub size: Option<Size<Points>>,
    /// The pixel densities a bundled asset ships in (`a.png`,
    /// `a@2x.png`, ...): the core mounts the one for the screen's pixel
    /// ratio. Empty for a single file.
    pub scales: ImageScales,
}

impl ImageSource {
    /// The URI to load at `pixel_ratio` device pixels per point: the
    /// variant [`ImageScales::pick`] chooses (`asset:/a.png` at 2 →
    /// `asset:/a@2x.png`), or [`Self::uri`] itself when there are no
    /// scales, the pick is the 1x file or the URI is not a plain
    /// `asset:/` path.
    pub(crate) fn uri_for(&self, pixel_ratio: f32) -> Arc<str> {
        match self.variant(pixel_ratio) {
            Some(scale) => variant_uri(&self.uri, scale).into(),
            None => self.uri.clone(),
        }
    }

    /// The scale of the variant loaded at `pixel_ratio`, when it is not
    /// [`Self::uri`] itself. A query or a fragment is no part of a file
    /// name the bundler wrote, so such a URI is loaded as it is.
    pub(crate) fn variant(&self, pixel_ratio: f32) -> Option<f32> {
        if self.scales.is_empty()
            || !self.uri.starts_with("asset:/")
            || self.uri.contains(['?', '#'])
        {
            return None;
        }
        self.scales.pick(pixel_ratio).filter(|s| *s != 1.0)
    }

    /// This source as the platform loads it at `pixel_ratio`: the chosen
    /// variant's URI and the same size in points.
    pub(crate) fn resolved(&self, pixel_ratio: f32) -> ImageSource {
        ImageSource {
            uri: self.uri_for(pixel_ratio),
            size: self.size,
            scales: ImageScales::NONE,
        }
    }
}

/// `asset:/dir/a.png` at 2 → `asset:/dir/a@2x.png` (`@1.5x` at 1.5).
fn variant_uri(uri: &str, scale: f32) -> String {
    let name = uri.rfind('/').map_or(0, |i| i + 1);
    let (stem, ext) = match uri[name..].rfind('.') {
        Some(dot) if dot > 0 => uri.split_at(name + dot),
        _ => (uri, ""),
    };
    format!("{stem}@{scale}x{ext}")
}

/// The pixel densities an image asset can ship in, as the file name
/// suffix says (`@1.5x` ... `@4x`; the file without a suffix is 1x). The
/// bundler looks for these siblings of an imported image.
pub const IMAGE_SCALES: [f32; 5] = [1.0, 1.5, 2.0, 3.0, 4.0];

/// A set of [`IMAGE_SCALES`]: the variants an image asset has. Collected
/// from scales (`[1.0, 2.0].into_iter().collect()`), it keeps those of
/// [`IMAGE_SCALES`] and leaves any other value (NaN, negative, `2.5`)
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ImageScales(u8);

impl ImageScales {
    /// No variants: the URI is the only file.
    pub const NONE: ImageScales = ImageScales(0);

    /// This set with `scale` added, when it is one of [`IMAGE_SCALES`].
    #[must_use]
    pub(crate) fn with(self, scale: f32) -> ImageScales {
        match IMAGE_SCALES.iter().position(|s| *s == scale) {
            Some(i) => ImageScales(self.0 | 1 << i),
            None => self,
        }
    }

    /// Whether the set has no scale.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The scales in the set, smallest first.
    pub fn iter(self) -> impl Iterator<Item = f32> {
        IMAGE_SCALES
            .into_iter()
            .enumerate()
            .filter(move |(i, _)| self.0 & (1 << i) != 0)
            .map(|(_, s)| s)
    }

    /// The scale to load at `pixel_ratio`, as React Native picks: the
    /// smallest that is at least the ratio, otherwise the largest. `None`
    /// for an empty set.
    pub(crate) fn pick(self, pixel_ratio: f32) -> Option<f32> {
        self.iter()
            .find(|s| *s >= pixel_ratio)
            .or_else(|| self.iter().last())
    }
}

impl FromIterator<f32> for ImageScales {
    fn from_iter<I: IntoIterator<Item = f32>>(values: I) -> ImageScales {
        values.into_iter().fold(Self::NONE, ImageScales::with)
    }
}

/// A non-style property of a host node.
#[derive(Debug, Clone, PartialEq)]
pub enum Prop {
    /// `src`: a URL or an imported asset.
    Source(Option<ImageSource>),
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
    /// Creates a detached raw text node. It is not a native view: inserted
    /// into a `<text>`, it is a run of that block.
    CreateText {
        /// New node id.
        id: NodeId,
        /// Text content.
        text: String,
    },
    /// Replaces the content of a raw text node.
    SetText {
        /// Text node id.
        id: NodeId,
        /// New content.
        text: String,
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
    /// Sets a non-style property.
    SetProp {
        /// Node id.
        id: NodeId,
        /// The property.
        prop: Prop,
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
