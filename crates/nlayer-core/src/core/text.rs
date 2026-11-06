//! Text blocks, attachments and link presses.

use super::*;

/// `align` in a layout of direction `rtl`: left and right trade places
/// right to left, as all left / right properties do.
pub(super) fn directed_align(align: TextAlign, rtl: bool) -> TextAlign {
    match (align, rtl) {
        (TextAlign::Left, true) => TextAlign::Right,
        (TextAlign::Right, true) => TextAlign::Left,
        (align, _) => align,
    }
}

/// The background a nested text's view style gives its characters.
pub(super) fn span_background(
    style: &ViewStyle,
    node: NodeId,
) -> Option<crate::text::TextBackground> {
    (style.background_color.0 & 0xff != 0).then_some(crate::text::TextBackground {
        color: style.background_color,
        radius: style.border_radius.max(Points::ZERO),
        node,
    })
}

impl<M: TextEngine> Core<M> {
    /// A short press on `id`: the innermost `href` at or around it is
    /// checked ([`crate::open_url::check_url`]) and, when accepted, mounted
    /// as [`Command::OpenUrl`] on the root. Returns the URL it opened.
    /// The press's own listeners still run; this does not change hit
    /// testing.
    pub fn press(&mut self, id: NodeId) -> Option<String> {
        let key = self.key(id).ok()?;
        let block = self.text_block_of(key)?;
        let layout = self.nodes[block as usize].layout?;
        let mut nodes = vec![id];
        let mut k = key;
        while k != block {
            k = self.nodes[k as usize].parent;
            if k == NO_KEY {
                break;
            }
            nodes.push(self.nodes[k as usize].id);
        }
        let url = {
            let LeafContext::Text(cx) = self.layout.get_node_context_mut(layout)? else {
                return None;
            };
            nodes
                .iter()
                .find_map(|&node| cx.text.href_of_node(node).map(|h| h.url.clone()))
        }?;
        self.open_checked(&url)
    }

    /// A short press at `point` (the text's content box, y down) on the
    /// text node `id`: the `href` at the offset the point hits, innermost
    /// first, then the ones around the node. Checked and mounted as
    /// [`Command::OpenUrl`] when accepted; returns the URL it opened.
    pub fn press_at(&mut self, id: NodeId, point: Point<Points>) -> Option<String> {
        let key = self.key(id).ok()?;
        let block = self.text_block_of(key)?;
        let block_id = self.nodes[block as usize].id;
        let layout = self.nodes[block as usize].layout?;
        let engine = &self.text_engine;
        let url = {
            let LeafContext::Text(cx) = self.layout.get_node_context_mut(layout)? else {
                return None;
            };
            let offset = cx
                .laid_out
                .as_ref()
                .map_or(0, |laid| engine.hit_test(&laid.layout, point).offset);
            cx.text
                .href_at(offset)
                .or_else(|| cx.text.href_of_node(id))
                .or_else(|| cx.text.href_of_node(block_id))
                .map(|h| h.url.clone())
        }?;
        self.open_checked(&url)
    }

    /// Checks `url` and mounts [`Command::OpenUrl`] for it. A refused URL
    /// is not opened, and why is logged; the press still reports itself.
    fn open_checked(&mut self, url: &Arc<str>) -> Option<String> {
        match crate::open_url::check_url(url) {
            Ok(()) => {
                let url = url.to_string();
                self.mount.push(MountItem::Command {
                    id: ROOT_ID,
                    command: Command::OpenUrl { url: url.clone() },
                });
                Some(url)
            }
            Err(reason) => {
                log::warn!("{reason}");
                None
            }
        }
    }

    /// The text block (not a span) around `key`.
    fn text_block_of(&self, mut key: Key) -> Option<Key> {
        loop {
            let node = &self.nodes[key as usize];
            if !node.inline {
                return (node.kind == Kind::Text).then_some(key);
            }
            if node.parent == NO_KEY {
                return None;
            }
            key = node.parent;
        }
    }

    /// Marks the text block containing `key` (or `key` itself) for rebuild.
    pub(super) fn mark_text_block(&mut self, mut key: Key) {
        loop {
            let node = &self.nodes[key as usize];
            if !node.inline {
                if node.kind == Kind::Text {
                    self.mark(key, dirty::TEXT_BUILD);
                }
                return;
            }
            if node.parent == NO_KEY {
                return;
            }
            key = node.parent;
        }
    }

    /// Text blocks inside `key` inherit its new text style.
    pub(super) fn mark_inheriting_text(&mut self, key: Key) {
        let mut stack: Vec<Key> = self.nodes[key as usize].children.clone();
        while let Some(k) = stack.pop() {
            match self.nodes[k as usize].kind {
                Kind::Text => self.mark_text_block(k),
                Kind::Input => {
                    self.measure_input(k);
                    self.mark(k, dirty::INPUT);
                }
                Kind::RawText => {}
                _ => stack.extend_from_slice(&self.nodes[k as usize].children),
            }
        }
    }

    pub(super) fn text_cx(&mut self, layout: LayoutId) -> &mut TextContext<M::Prepared> {
        match self.layout.get_node_context_mut(layout) {
            Some(LeafContext::Text(cx)) => cx,
            _ => unreachable!("text block without text context"),
        }
    }

    /// The style text inherits at the root: React Native's defaults.
    fn base_text_style(&self) -> ResolvedTextStyle {
        ResolvedTextStyle::default()
    }

    /// The text style `key` inherits from the views around it, as CSS
    /// does: `color` on a container styles the text in it.
    pub(super) fn inherited_text_style(&self, key: Key) -> ResolvedTextStyle {
        let mut chain: Vec<Key> = Vec::new();
        let mut k = self.nodes[key as usize].parent;
        while k != NO_KEY {
            let node = &self.nodes[k as usize];
            if node.kind != Kind::Text && node.text_style != TextStyle::default() {
                chain.push(k);
            }
            k = node.parent;
        }
        chain
            .iter()
            .rev()
            .fold(self.base_text_style(), |style, &k| {
                style.inherit(&self.nodes[k as usize].text_style)
            })
    }

    /// The nearest `textAlign` set on a view around `key`.
    fn inherited_text_align(&self, key: Key) -> Option<(TextAlign, bool)> {
        let mut k = self.nodes[key as usize].parent;
        while k != NO_KEY {
            let node = &self.nodes[k as usize];
            if let Some(align) = node.text_style.text_align {
                return Some((align, node.text_style.text_align_physical));
            }
            k = node.parent;
        }
        None
    }

    /// Rebuilds the attributed text of a block. Returns true if it changed.
    ///
    /// Only layout-relevant changes invalidate measurement and layout; a
    /// color change keeps both and just needs a new final layout.
    pub(super) fn rebuild_text(&mut self, key: Key) -> bool {
        let node = &self.nodes[key as usize];
        let max_lines = node.number_of_lines.unwrap_or(0);
        let ellipsize = node.ellipsize.unwrap_or_default();
        let inherited = self.inherited_text_style(key);
        let selectable = self.resolved_user_select(key);
        let (align, physical) = node
            .text_style
            .text_align
            .map(|align| (align, node.text_style.text_align_physical))
            .or_else(|| self.inherited_text_align(key))
            .unwrap_or_default();
        let mut text = AttributedText {
            align: directed_align(align, node.rtl && !physical),
            writing_direction: node.text_style.writing_direction,
            rtl: node.rtl,
            max_lines,
            ellipsize,
            shadow: node.text_style.shadow(),
            selectable,
            selection_color: node
                .text_style
                .selection_color
                .or_else(|| node.class_props.as_ref().and_then(|p| p.selection_color)),
            ..AttributedText::default()
        };
        let base = inherited.inherit(&node.text_style);
        let inserted = self.update_attachments(key);
        let mut alignments = AttachmentAlignments::default();
        self.collect_text(key, base, &mut text, &mut alignments);
        // Its own `href` covers everything it draws.
        if let Some(url) = self.nodes[key as usize].href.clone() {
            text.hrefs.push(TextHref {
                start: 0,
                end: text.len_utf16(),
                node: self.nodes[key as usize].id,
                url,
            });
        }
        let layout = self.nodes[key as usize]
            .layout
            .expect("text block has layout");
        let cx = self.text_cx(layout);
        if *cx.text == text && cx.alignments == alignments {
            if inserted {
                // New attachment views: placed with the next final layout.
                cx.laid_out = None;
            }
            return inserted;
        }
        // A test id is not a glyph: keep the layout and remount the text.
        if cx.text.test_ids != text.test_ids
            && !inserted
            && cx.alignments == alignments
            && cx.text.rendering_eq(&text)
        {
            cx.text = Arc::new(text);
            cx.metadata_changed = true;
            return true;
        }
        cx.metadata_changed = false;
        let same_layout = cx.text.layout_eq(&text) && cx.alignments == alignments;
        cx.alignments = alignments;
        cx.text = Arc::new(text);
        cx.prepared = None;
        cx.min_content = None;
        cx.laid_out = None;
        if !same_layout {
            let has_alignments = !cx.alignments.is_empty();
            cx.layout_hash = cx.alignments.layout_hash(&cx.text);
            cx.cache = None;
            // A text that measures as before leaves every layout standing:
            // only the text is mounted again. Attachments are placed by its
            // final layout, laid out again with the rest.
            let attachments = has_alignments || !self.nodes[key as usize].attached.is_empty();
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            if attachments
                || !self
                    .layout
                    .measures_as_before(layout, |input, _, cx, style| {
                        super::layout::measure_leaf(engine, cache, input, cx, style)
                    })
            {
                self.layout.mark_dirty(layout);
                self.needs_layout = true;
            }
        }
        true
    }

    /// The `userSelect` of `key` and the views around it: `Some(true)` is
    /// `"text"`, `Some(false)` is `"none"`, `None` is `"auto"` everywhere.
    fn resolved_user_select(&self, key: Key) -> Option<bool> {
        let mut k = key;
        while k != NO_KEY {
            if let Some(selectable) = self.nodes[k as usize].user_select {
                return Some(selectable);
            }
            k = self.nodes[k as usize].parent;
        }
        None
    }

    /// Collects `key`'s characters into `out`: raw text, its spans and
    /// their `href`s, and its attachments' placeholders.
    fn collect_text(
        &self,
        key: Key,
        style: ResolvedTextStyle,
        out: &mut AttributedText,
        alignments: &mut AttachmentAlignments,
    ) {
        for &c in &self.nodes[key as usize].children {
            let child = &self.nodes[c as usize];
            match child.kind {
                Kind::RawText => out.push(&child.text, style),
                Kind::Text => {
                    let start = out.len_utf16();
                    let mut inner = style.inherit(&child.text_style);
                    if let Some(background) = span_background(&child.view_style, child.id) {
                        inner.background = Some(background);
                    }
                    let events = child.events;
                    let id = child.id;
                    let test_id = child.test_id.clone();
                    let href = child.href.clone();
                    self.collect_text(c, inner, out, alignments);
                    let end = out.len_utf16();
                    if events.contains(EventKind::Press) {
                        out.links.push(crate::text::TextLink {
                            start,
                            end,
                            node: id,
                            events,
                        });
                    }
                    if let Some(url) = href {
                        out.hrefs.push(TextHref {
                            start,
                            end,
                            node: id,
                            url,
                        });
                    }
                    if let Some(test_id) = test_id {
                        out.test_ids.push(SpanTestId {
                            start,
                            end,
                            test_id,
                        });
                    }
                }
                // An attachment, laid out by `update_attachments`: one
                // placeholder of its own size.
                _ => {
                    if let Some(layout) = child.layout {
                        let size = self.rounded_layout(layout).size;
                        alignments.push(
                            out,
                            crate::geometry::size_pt(size.width, size.height),
                            child.vertical_align,
                            style,
                        );
                    }
                }
            }
        }
    }

    /// The attachments of text block `key` (views and images inside it,
    /// also inside its spans), in text order.
    fn attachments_of(&self, key: Key, out: &mut Vec<Key>) {
        for &c in &self.nodes[key as usize].children {
            let child = &self.nodes[c as usize];
            match child.kind {
                Kind::RawText => {}
                Kind::Text => self.attachments_of(c, out),
                _ if child.layout.is_some() => out.push(c),
                _ => {}
            }
        }
    }

    /// The text block an attachment is in, if it is one.
    fn attachment_block(&self, key: Key) -> Option<Key> {
        let node = &self.nodes[key as usize];
        if !node.inline || node.layout.is_none() || node.kind == Kind::Text {
            return None;
        }
        let mut k = node.parent;
        while k != NO_KEY {
            let n = &self.nodes[k as usize];
            if !n.inline {
                return (n.kind == Kind::Text).then_some(k);
            }
            k = n.parent;
        }
        None
    }

    /// Lays out the attachments of text block `key` (each a layout root at
    /// its own content size) and keeps their native views inside the
    /// block's. Returns true if views were inserted (they need placing).
    fn update_attachments(&mut self, key: Key) -> bool {
        let mut keys = Vec::new();
        self.attachments_of(key, &mut keys);
        let block = &self.nodes[key as usize];
        if keys.is_empty() && block.attached.is_empty() {
            return false;
        }
        let (block_id, created) = (block.id, block.created);
        let old = mem::take(&mut self.nodes[key as usize].attached);
        let mut inserted = false;
        for &a in &keys {
            let id = self.nodes[a as usize].id;
            let new = !old.contains(&id);
            if created && new {
                self.ensure_created(a);
                self.mount.push(MountItem::Insert {
                    parent: block_id,
                    child: id,
                    before: None,
                });
                inserted = true;
            }
            let layout = self.nodes[a as usize].layout.expect("attachment layout");
            let dirty = self.layout.dirty(layout);
            if dirty {
                self.layout_root(
                    layout,
                    taffy::Size {
                        width: AvailableSpace::MaxContent,
                        height: AvailableSpace::MaxContent,
                    },
                );
            }
            if created && (dirty || new) {
                // Its own frame follows the text; its subtree's now.
                let children = mem::take(&mut self.nodes[a as usize].children);
                for &child in &children {
                    self.emit_frames(child);
                }
                self.nodes[a as usize].children = children;
            }
        }
        // Views that left the block and were not moved natively elsewhere.
        for id in old {
            let Ok(k) = self.key(id) else { continue };
            let n = &self.nodes[k as usize];
            let moved = n.parent != NO_KEY && !n.inline && self.nodes[n.parent as usize].created;
            if n.created && !moved && self.attachment_block(k).is_none() {
                self.mount.push(MountItem::Remove { id });
            }
        }
        let ids: Vec<NodeId> = keys.iter().map(|&a| self.nodes[a as usize].id).collect();
        let has = !ids.is_empty();
        self.nodes[key as usize].attached = if created { ids } else { Vec::new() };
        match (has, self.attachment_blocks.contains(&key)) {
            (true, false) => self.attachment_blocks.push(key),
            (false, true) => self.attachment_blocks.retain(|&k| k != key),
            _ => {}
        }
        inserted
    }

    /// Text blocks whose attachments' content changed are built again.
    pub(super) fn check_attachments(&mut self) {
        let mut i = 0;
        while i < self.attachment_blocks.len() {
            let key = self.attachment_blocks[i];
            i += 1;
            let Some(node) = self.nodes.get(key as usize) else {
                continue;
            };
            let dirty = node.attached.iter().any(|&id| {
                self.key(id).is_ok_and(|k| {
                    self.nodes[k as usize]
                        .layout
                        .is_some_and(|l| self.layout.dirty(l))
                })
            });
            if dirty {
                self.mark(key, dirty::TEXT_BUILD);
            }
        }
    }

    /// Moves the views of text block `key`'s attachments onto their
    /// placeholders in `layout` (content box `content`); cut off ones out
    /// of sight.
    fn place_attachments(&mut self, key: Key, layout: &TextLayout, content: Bounds<Points>) {
        let points = self.text_engine.attachments(layout);
        let attached = mem::take(&mut self.nodes[key as usize].attached);
        for (i, &id) in attached.iter().enumerate() {
            let Ok(a) = self.key(id) else { continue };
            let Some(l) = self.nodes[a as usize].layout else {
                continue;
            };
            let size = self.rounded_layout(l).size;
            let (x, y) = match points.get(i).copied().flatten() {
                Some(p) => (content.origin.x.0 + p.x.0, content.origin.y.0 + p.y.0),
                // Far away, like unbound list cells.
                None => (-100_000.0, 0.0),
            };
            self.set_frame(a, rect(x, y, size.width, size.height));
        }
        self.nodes[key as usize].attached = attached;
    }

    #[inline]
    pub(super) fn queue_text(&mut self, key: Key) {
        let node = &mut self.nodes[key as usize];
        if !node.text_queued {
            node.text_queued = true;
            self.pending_text.push(key);
        }
    }

    /// Makes the final layout of every queued text block whose content or
    /// box changed, and emits it.
    pub(super) fn flush_text(&mut self) {
        let pending = mem::take(&mut self.pending_text);
        let mut placements: Vec<(Key, TextLayout, Bounds<Points>)> = Vec::new();
        for key in pending {
            let (align_vertical, id, layout) = {
                let Some(node) = self.nodes.get_mut(key as usize) else {
                    continue;
                };
                node.text_queued = false;
                let align_vertical = node.text_style.align_vertical.unwrap_or_default();
                if !node.created {
                    continue;
                }
                let Some(layout) = node.layout else {
                    continue;
                };
                (align_vertical, node.id, layout)
            };
            let hidden = self.displays_none(key);
            let mut content = if hidden {
                Bounds::default()
            } else {
                super::layout::content_box(&self.rounded_layout(layout))
            };
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout) else {
                continue;
            };
            if hidden {
                // Out of the layout (`display: none`). Glyphs typeset in
                // its zero box would still show where a platform does not
                // clip text to its box, so it gets an empty layout; the
                // text itself stays what it is.
                if matches!(&cx.laid_out, Some(l) if l.content == content) {
                    continue;
                }
                let text_layout =
                    engine.layout(&engine.prepare(&AttributedText::default()), content.size);
                cx.laid_out = Some(LaidText {
                    content,
                    layout: text_layout.clone(),
                });
                self.mount.push(MountItem::Text {
                    id,
                    text: cx.text.clone(),
                    layout: text_layout,
                    content,
                });
                continue;
            }
            if cx.text.no_wrap {
                content.size.width = content
                    .size
                    .width
                    .max(super::layout::measure_text(engine, cache, cx, None).width);
            }
            if align_vertical > Percentage(0.0) {
                let height =
                    super::layout::measure_text(engine, cache, cx, Some(content.size.width.0))
                        .height;
                if height < content.size.height {
                    content.origin.y += align_vertical.of(content.size.height - height);
                    content.size.height = height;
                }
            }
            if cx.metadata_changed {
                let reuse = cx
                    .laid_out
                    .as_ref()
                    .filter(|laid| laid.content == content)
                    .map(|laid| laid.layout.clone());
                if let Some(layout) = reuse {
                    let text = cx.text.clone();
                    cx.metadata_changed = false;
                    self.mount.push(MountItem::Text {
                        id,
                        text,
                        layout,
                        content,
                    });
                    continue;
                }
                cx.metadata_changed = false;
            }
            let size = content.size;
            let text_layout = match &cx.laid_out {
                Some(l) if l.content == content => continue,
                Some(l) if l.content.size == size => l.layout.clone(),
                _ => {
                    let prepared = cx
                        .prepared
                        .get_or_insert_with(|| engine.prepare_aligned(&cx.text, &cx.alignments));
                    engine.layout(prepared, size)
                }
            };
            cx.laid_out = Some(LaidText {
                content,
                layout: text_layout.clone(),
            });
            if !cx.text.attachments.is_empty() {
                placements.push((key, text_layout.clone(), content));
            }
            self.mount.push(MountItem::Text {
                id,
                text: cx.text.clone(),
                layout: text_layout,
                content,
            });
        }
        for (key, layout, content) in placements {
            self.place_attachments(key, &layout, content);
        }
        // A layout that did not rebuild the text still walked these views
        // (`display: none` zeroes them). Put back the placeholders of the
        // blocks that are showing.
        self.restore_attachments();
    }

    /// Puts every showing text block's attachments back on the placeholders
    /// of the layout already sent.
    fn restore_attachments(&mut self) {
        let blocks = self.attachment_blocks.clone();
        for key in blocks {
            if self.displays_none(key) {
                continue;
            }
            let Some(layout_id) = self.nodes.get(key as usize).and_then(|node| node.layout) else {
                continue;
            };
            let placed = match self.layout.get_node_context_mut(layout_id) {
                Some(LeafContext::Text(cx)) => cx
                    .laid_out
                    .as_ref()
                    .map(|laid| (laid.layout.clone(), laid.content)),
                _ => None,
            };
            if let Some((layout, content)) = placed {
                self.place_attachments(key, &layout, content);
            }
        }
    }
}
