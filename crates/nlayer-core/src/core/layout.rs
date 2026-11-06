//! Taffy passes, frames and text measurement for layout.

use super::*;

pub(super) fn measure_leaf<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    input: taffy::LayoutInput,
    cx: Option<&mut LeafContext<M::Prepared>>,
    style: &TaffyStyle,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(
        input,
        style,
        |_, _| 0.0,
        |known, available| match cx {
            Some(LeafContext::Text(cx)) => {
                let max_width = if cx.text.no_wrap {
                    None
                } else {
                    known.width.or(match available.width {
                        AvailableSpace::Definite(w) => Some(w),
                        AvailableSpace::MinContent => Some(min_content_width(engine, cache, cx).0),
                        AvailableSpace::MaxContent => None,
                    })
                };
                let measured = measure_text(engine, cache, cx, max_width);
                taffy::Size {
                    width: known.width.unwrap_or(measured.width.0),
                    height: known.height.unwrap_or(measured.height.0),
                }
            }
            Some(LeafContext::Image(Some(natural))) => {
                image_size(*natural, known.width, known.height)
            }
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

/// The content box (inside padding and border) of a laid-out node.
pub(super) fn content_box(l: &taffy::Layout) -> Bounds<Points> {
    rect(
        l.padding.left + l.border.left,
        l.padding.top + l.border.top,
        (l.size.width - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0),
        (l.size.height - l.padding.top - l.padding.bottom - l.border.top - l.border.bottom)
            .max(0.0),
    )
}

/// Min-content keeps every unbreakable segment intact. Measure the styled
/// segments with the platform engine rather than estimating glyph advances.
fn min_content_width<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    cx: &mut TextContext<M::Prepared>,
) -> Points {
    if let Some(width) = cx.min_content {
        return width;
    }
    let text = &cx.text;
    let mut offsets = Vec::with_capacity(text.text.chars().count() + 1);
    let mut utf16 = 0u32;
    for (byte, ch) in text.text.char_indices() {
        offsets.push((utf16, byte));
        utf16 += ch.len_utf16() as u32;
    }
    offsets.push((utf16, text.text.len()));
    let byte_at = |offset: u32| {
        offsets[offsets
            .binary_search_by_key(&offset, |&(at, _)| at)
            .unwrap()]
        .1
    };
    let mut start = 0usize;
    let mut width = Points::ZERO;
    for (end, _) in unicode_linebreak::linebreaks(&text.text) {
        let word = text.text[start..end].trim_end_matches([' ', '\t', '\n', '\r', '\u{000c}']);
        let from = offsets[offsets
            .binary_search_by_key(&start, |&(_, byte)| byte)
            .unwrap()]
        .0;
        let to = from + word.encode_utf16().count() as u32;
        if from < to {
            let mut part = AttributedText {
                no_wrap: true,
                rtl: text.rtl,
                writing_direction: text.writing_direction,
                ..Default::default()
            };
            for span in &text.spans {
                let (lo, hi) = (from.max(span.start), to.min(span.end));
                if lo < hi {
                    part.push(&text.text[byte_at(lo)..byte_at(hi)], span.style);
                }
            }
            for attachment in &text.attachments {
                if (from..to).contains(&attachment.index) {
                    let mut attachment = *attachment;
                    attachment.index -= from;
                    part.attachments.push(attachment);
                }
            }
            let hash = part.layout_hash();
            let measured = cache.get(hash, None).unwrap_or_else(|| {
                let measured = engine.measure(&engine.prepare(&part), None);
                cache.insert(hash, None, measured);
                measured
            });
            width = width.max(measured.width);
        }
        start = end;
    }
    cx.min_content = Some(width);
    width
}

pub(super) fn measure_text<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    cx: &mut TextContext<M::Prepared>,
    max_width: Option<f32>,
) -> Size<Points> {
    if let Some((w, size)) = cx.cache
        && w == max_width
    {
        return size;
    }
    let typed = max_width.map(Points);
    let size = match cache.get(cx.layout_hash, typed) {
        Some(size) => size,
        None => {
            let prepared = cx
                .prepared
                .get_or_insert_with(|| engine.prepare_aligned(&cx.text, &cx.alignments));
            let size = engine.measure(prepared, typed);
            // Round up so rendering never truncates the last glyph.
            let size = size.map(Points::ceil);
            cache.insert(cx.layout_hash, typed, size);
            size
        }
    };
    cx.cache = Some((max_width, size));
    size
}

pub(super) fn root_style(viewport: Size<Points>) -> TaffyStyle {
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

impl<M: TextEngine> Core<M> {
    /// Lays the changed trees out and emits their frames.
    pub(super) fn layout_round(&mut self) {
        if mem::take(&mut self.needs_layout) {
            self.layout_once();
            // Rounding can make a text's box narrower than the width it was
            // measured at. One more pass gives that line a row, then stops.
            if self.grow_clipped_text() {
                self.layout_once();
            }
        }
        self.layout_cells();
    }

    /// Lays out what is dirty and emits the frames that moved.
    fn layout_once(&mut self) {
        let relaid = self.relayout_boundaries();
        if self.layout_main() {
            let root = self.root;
            let children = mem::take(&mut self.nodes[root as usize].children);
            for &child in &children {
                if !self.nodes[child as usize].kind.is_layout_root() {
                    self.emit_frames(child);
                }
            }
            self.nodes[root as usize].children = children;
        }
        self.layout_roots();
        for key in relaid {
            self.emit_frames(key);
        }
    }

    /// Texts whose drawn box is narrower than the width they were measured
    /// at, and whose line at that box is taller than the frame. Gives each
    /// a minimum height and marks it dirty. A fixed height is left alone.
    fn grow_clipped_text(&mut self) -> bool {
        let texts: Vec<Key> = self
            .nodes
            .iter()
            .filter_map(|(index, node)| {
                (node.kind == Kind::Text && node.created && node.layout.is_some())
                    .then_some(index as Key)
            })
            .collect();
        let mut grew = false;
        for key in texts {
            if self.displays_none(key) {
                continue;
            }
            let Some(layout_id) = self.nodes[key as usize].layout else {
                continue;
            };
            let content = content_box(&self.rounded_layout(layout_id));
            let measured = {
                let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout_id)
                else {
                    continue;
                };
                cx.cache
            };
            let Some((measured_at, measured_size)) = measured else {
                continue;
            };
            // Flex measures the line once at the border box (one line) and
            // again at the content box (wrapped). The used height stays the
            // first, and the cache only keeps the second width, so a matching
            // width is not proof the frame holds the line.
            let width_matches =
                measured_at.is_some_and(|width| content.size.width.0 + 0.05 >= width);
            if width_matches
                && measured_size.height.0 <= content.size.height.0 + 0.5
                && self.nodes[key as usize].line_floor.is_none()
            {
                continue;
            }
            if !self.layout.style(layout_id).size.height.is_auto() {
                continue;
            }
            let height = if width_matches {
                measured_size.height
            } else {
                let engine = &self.text_engine;
                let cache = &mut self.measure_cache;
                let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout_id)
                else {
                    continue;
                };
                measure_text(engine, cache, cx, Some(content.size.width.0)).height
            };
            let unrounded = *self.layout.unrounded_layout(layout_id);
            let pad = unrounded.padding.top
                + unrounded.padding.bottom
                + unrounded.border.top
                + unrounded.border.bottom;
            let needed = height.0 + pad;
            let floor = self.nodes[key as usize].line_floor;
            let mut style = self.layout.style(layout_id).clone();
            let (applied, restored) = match floor {
                Some((applied, previous))
                    if style.min_size.height.expand()
                        == taffy::LengthPercentageAuto::length(applied).expand() =>
                {
                    (Some(applied), previous)
                }
                _ => (None, style.min_size.height),
            };
            if needed > unrounded.size.height + 0.5 {
                let min = match restored.expand() {
                    taffy::ExpandedLengthPercentageAuto::Length(len) => len.max(needed),
                    _ => needed,
                };
                style.min_size.height = taffy::LengthPercentageAuto::length(min);
                self.layout.set_style(layout_id, style);
                self.layout.mark_dirty(layout_id);
                self.nodes[key as usize].line_floor = Some((min, restored));
                grew = true;
            } else if applied.is_some_and(|applied| needed + 0.5 < applied) {
                style.min_size.height = restored;
                self.layout.set_style(layout_id, style);
                self.layout.mark_dirty(layout_id);
                self.nodes[key as usize].line_floor = None;
                grew = true;
            } else if floor.is_some() && applied.is_none() {
                self.nodes[key as usize].line_floor = None;
            }
        }
        grew
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

    /// `layout`'s layout, rounded to whole points as Taffy's rounding pass
    /// does ([`rounded`]).
    pub(super) fn rounded_layout(&self, layout: LayoutId) -> taffy::Layout {
        rounded(
            self.layout.unrounded_layout(layout),
            self.layout_origin(layout),
        )
    }

    fn layout_main(&mut self) -> bool {
        let root = self.nodes[self.root as usize]
            .layout
            .expect("root has a layout");
        let available = taffy::Size {
            width: AvailableSpace::Definite(self.viewport.width.0),
            height: AvailableSpace::Definite(self.viewport.height.0),
        };
        let main = self.layout.dirty(root);
        if main {
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            self.layout
                .compute_layout_with_measure(root, available, |input, _, cx, style| {
                    measure_leaf(engine, cache, input, cx, style)
                });
        }
        main
    }

    fn relayout_boundaries(&mut self) -> Vec<Key> {
        let engine = &self.text_engine;
        let cache = &mut self.measure_cache;
        self.layout.relayout_boundaries(|input, _, cx, style| {
            measure_leaf(engine, cache, input, cx, style)
        })
    }

    pub(super) fn displays_none(&self, key: Key) -> bool {
        let mut k = key;
        while k != NO_KEY {
            if let Some(layout) = self.nodes[k as usize].layout
                && self.layout.style(layout).display == taffy::Display::None
            {
                return true;
            }
            k = self.nodes[k as usize].parent;
        }
        false
    }

    pub(super) fn emit_frames(&mut self, key: Key) {
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        let parent = self.nodes[key as usize].parent;
        let hidden = parent != NO_KEY && self.displays_none(parent);
        let origin = self
            .layout
            .parent(layout)
            .map_or(taffy::Point::ZERO, |parent| {
                let origin = self.layout_origin(parent);
                let location = self.layout.unrounded_layout(parent).location;
                taffy::Point {
                    x: origin.x + location.x,
                    y: origin.y + location.y,
                }
            });
        self.emit_frames_in(key, hidden, origin);
    }

    fn emit_frames_in(&mut self, key: Key, hidden: bool, origin: taffy::Point<f32>) {
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
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
        let mut frame = rect(
            l.location.x + round(containing.x) - round(origin.x),
            l.location.y + round(containing.y) - round(origin.y),
            l.size.width,
            l.size.height,
        );
        // Children of a right-to-left horizontal scroll view are mirrored
        // in its content. The view is laid out left to right, so a row
        // does not overflow to negative offsets.
        let parent = self.nodes[key as usize].parent;
        if parent != NO_KEY {
            let parent = &self.nodes[parent as usize];
            if parent.rtl
                && parent.kind == Kind::Scroll
                && let Some(content) = parent
                    .scroll
                    .as_ref()
                    .filter(|scroll| scroll.horizontal)
                    .and_then(|scroll| scroll.content)
            {
                frame.origin.x = content.width - frame.origin.x - frame.size.width;
            }
        }
        // An attachment's frame is where `place_attachments` put its
        // placeholder. Its taffy node is its own root, so the location is
        // (0, 0); writing that on a later layout stacks every inline view
        // at the text's origin.
        let follows_text =
            self.nodes[key as usize].inline && self.nodes[key as usize].kind != Kind::Text;
        if !follows_text {
            self.set_frame(key, frame);
        }
        if self.nodes[key as usize].kind == Kind::Input {
            let content = content_box(&l);
            let node = &mut self.nodes[key as usize];
            let input = node.input.as_mut().expect("input state");
            if input.content != Some(content) {
                input.content = Some(content);
                self.mount.push(MountItem::InputContent {
                    id: node.id,
                    content,
                });
            }
        }
        if self.nodes[key as usize].kind == Kind::Scroll {
            // Scrollable extent. Taffy 0.15 already adds a scroll container's
            // end padding to `scrollable_overflow_rect`: the rect is measured
            // from the padding edge, stops at the children's border boxes,
            // then grows by that padding. Adding it again made the content
            // long by the padding (a safe-area `env()` included). The leading
            // border is still outside the rect.
            let o = l.scrollable_overflow_rect;
            let size = crate::geometry::size_pt(
                l.size.width.max(l.border.left + o.right),
                l.size.height.max(l.border.top + o.bottom),
            );
            self.set_scroll_content(key, size);
        }
        let children = mem::take(&mut self.nodes[key as usize].children);
        for &child in &children {
            self.emit_frames_in(child, false, inner);
        }
        self.nodes[key as usize].children = children;
    }

    pub(super) fn set_frame(&mut self, key: Key, frame: Bounds<Points>) {
        let (id, is_text, restyle, fractional) = {
            let node = &mut self.nodes[key as usize];
            if node.frame == frame {
                return;
            }
            node.frame = frame;
            let fractional: Vec<usize> = node
                .state
                .iter()
                .enumerate()
                .filter_map(|(slot, paint)| {
                    paint
                        .as_ref()
                        .is_some_and(|paint| paint.view.transform.has_fractions())
                        .then_some(slot)
                })
                .collect();
            (
                node.id,
                node.kind == Kind::Text,
                node.view_style.transform.has_fractions(),
                fractional,
            )
        };
        self.mount.push(MountItem::Frame { id, frame });
        if restyle {
            let style = self.mounted_view_style(key);
            self.mount.push(MountItem::ViewStyle { id, style });
        }
        for slot in fractional {
            self.emit_state(key, id, slot);
        }
        // The content box is inside this frame. A size or padding change
        // does not rebuild the string, so the text is sent again from here.
        if is_text {
            self.queue_text(key);
        }
    }

    /// Lays out one layout root (Screen, Modal, HeaderItem) at the size
    /// its container gives.
    pub(super) fn layout_detached(&mut self, key: Key) {
        let native_header = self.native_header();
        let drawn_header = if !native_header && self.nodes[key as usize].kind == Kind::Screen {
            self.drawn_header(key)
        } else {
            0.0
        };
        let node = &self.nodes[key as usize];
        let (Some(layout), kind, parent) = (node.layout, node.kind, node.parent) else {
            return;
        };
        let root = node.root.as_ref().expect("root state");
        let (size, header, available) = match kind {
            Kind::Screen => {
                let Some(size) = self.screen_size(key) else {
                    return;
                };
                let c = &root.screen;
                let measured = root.header.filter(|h| *h > 0.0 && !c.header_transparent);
                let bar = c.content_bar(self.has_title_view(key)).0;
                let header = if !native_header {
                    drawn_header
                } else if !c.header_visible {
                    0.0
                } else if let Some(measured) = measured {
                    measured
                } else {
                    bar + self.top_overlap(parent)
                };
                (
                    size,
                    header,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(size.width.0),
                        height: taffy::AvailableSpace::Definite(size.height.0),
                    },
                )
            }
            Kind::HeaderItem if !native_header && root.slot == crate::ops::HeaderSlot::Header => {
                let Some(screen) = self.screen_size(parent) else {
                    return;
                };
                (
                    crate::geometry::size_pt(screen.width.0, 0.0),
                    0.0,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(screen.width.0),
                        height: taffy::AvailableSpace::MaxContent,
                    },
                )
            }
            Kind::Modal => {
                let size = root.reported.unwrap_or(self.viewport);
                (
                    size,
                    0.0,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(size.width.0),
                        height: taffy::AvailableSpace::Definite(size.height.0),
                    },
                )
            }
            // A header item of a native header: the platform places it,
            // but its content still lays out for the frame it reports.
            _ => (
                crate::geometry::Size::default(),
                0.0,
                taffy::Size {
                    width: taffy::AvailableSpace::MaxContent,
                    height: taffy::AvailableSpace::MaxContent,
                },
            ),
        };
        let changed = root.laid_out != Some((size, header));
        if !changed && !self.layout.dirty(layout) {
            return;
        }
        if kind == Kind::HeaderItem && size.width > Points::ZERO {
            let mut style = self.layout.style(layout).clone();
            style.size.width = taffy::Dimension::length(size.width.0);
            style.size.height = taffy::Dimension::auto();
            self.layout.set_style(layout, style);
        } else if kind != Kind::HeaderItem {
            let config_padding = match kind {
                // A transparent header stays on top: the system bar on
                // iOS and Android, the app's `header` slot on the web.
                // Content starts at the top and shows through it.
                Kind::Screen if root.screen.header_transparent => 0.0,
                _ => header,
            };
            let mut style = self.nodes[key as usize]
                .base_layout
                .as_deref()
                .cloned()
                .unwrap_or_default();
            style.size = taffy::Size {
                width: taffy::Dimension::length(size.width.0),
                height: taffy::Dimension::length(size.height.0),
            };
            if config_padding > 0.0 {
                let base = match style.padding.top.into_raw() {
                    raw if raw.tag() == taffy::CompactLength::LENGTH_TAG => raw.value(),
                    _ => 0.0,
                };
                style.padding.top = taffy::LengthPercentage::length(base + config_padding);
            }
            self.layout.set_style(layout, style);
        }
        let header_changed = root.laid_out.is_none_or(|(_, h)| h != header);
        self.nodes[key as usize]
            .root
            .as_mut()
            .expect("root state")
            .laid_out = Some((size, header));
        self.layout_root(layout, available);
        self.emit_frames(key);
        if kind == Kind::Screen && header_changed {
            let id = self.nodes[key as usize].id;
            if let Some(config) = self.screen_config(key) {
                self.mount.push(MountItem::ScreenConfig {
                    id,
                    config: Box::new(config),
                });
            }
        }
    }

    /// Lays out the tree under `root` within `available`.
    pub(super) fn layout_root(
        &mut self,
        root: LayoutId,
        available: taffy::Size<taffy::AvailableSpace>,
    ) {
        let engine = &self.text_engine;
        let cache = &mut self.measure_cache;
        self.layout
            .compute_layout_with_measure(root, available, |input, _, cx, style| {
                measure_leaf(engine, cache, input, cx, style)
            });
    }

    /// Lays out layout roots (screens, modals, header items) in depth order.
    fn layout_roots(&mut self) {
        if self.roots.is_empty() {
            return;
        }
        let mut order: Vec<(u32, Key)> = self
            .roots
            .iter()
            .filter(|&&k| {
                let n = &self.nodes[k as usize];
                n.created && n.parent != NO_KEY
            })
            .map(|&k| (self.root_depth(k), k))
            .collect();
        order.sort_unstable();
        for (_, key) in order {
            self.layout_detached(key);
        }
    }

    /// Lays out the list cells that changed: items at their column's
    /// width, the header, footer and empty placeholder at the full width.
    fn layout_cells(&mut self) {
        for i in 0..self.lists.len() {
            let key = self.lists[i];
            let Some(list_layout) = self.nodes.get(key as usize).and_then(|n| n.layout) else {
                continue;
            };
            if !self.nodes[key as usize].created {
                continue;
            }
            let size = self.rounded_layout(list_layout).size;
            let cross = size.width;
            let state = self.nodes[key as usize].list.as_mut().expect("list");
            let resized = state.cells_cross != cross;
            state.cells_cross = cross;
            let item_cross = state.column(cross, 0).1;
            let extra_cross = state.full_width(cross).1;
            let cells = std::mem::take(&mut self.nodes[key as usize].children);
            for &cell in &cells {
                let node = &self.nodes[cell as usize];
                let Some(cell_layout) = node.layout else {
                    continue;
                };
                let width = match node.slot {
                    Some(CellSlot::Item(_)) | None => item_cross,
                    Some(_) => extra_cross,
                };
                if resized || self.layout.dirty(cell_layout) {
                    let available = taffy::Size {
                        width: taffy::AvailableSpace::Definite(width),
                        height: taffy::AvailableSpace::MaxContent,
                    };
                    let engine = &self.text_engine;
                    let cache = &mut self.measure_cache;
                    self.layout.compute_layout_with_measure(
                        cell_layout,
                        available,
                        |input, _, cx, style| measure_leaf(engine, cache, input, cx, style),
                    );
                    self.nodes[cell as usize].relaid = true;
                }
            }
            self.nodes[key as usize].children = cells;
        }
    }
}
