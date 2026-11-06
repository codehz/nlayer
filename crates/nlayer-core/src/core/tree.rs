//! Create, insert, move and release shadow nodes.

use super::*;

impl<M: TextEngine> Core<M> {
    /// Applies one op from JS.
    pub fn apply(&mut self, op: Op) -> Result<(), ApplyError> {
        match op {
            Op::Create { id, tag } => self.create(id, tag),
            Op::CreateText { id, text } => {
                let key = self.alloc(id, Kind::RawText)?;
                self.nodes[key as usize].text = text;
                Ok(())
            }
            Op::SetText { id, text } => {
                let key = self.key(id)?;
                let node = &mut self.nodes[key as usize];
                if node.text != text {
                    node.text = text;
                    if node.inline {
                        self.mark_text_block(key);
                    }
                }
                Ok(())
            }
            Op::Animation(op) => {
                self.mount.push(MountItem::Animation(op));
                Ok(())
            }
            Op::SetStyle { id, style, patch } => self.apply_authored_style(id, *style, patch),
            Op::SetProp { id, prop } => self.set_prop(id, prop),
            Op::Listen { id, event, enabled } => {
                let key = self.key(id)?;
                let node = &mut self.nodes[key as usize];
                let before = node.events;
                node.events.set(event, enabled);
                if node.events != before {
                    // A span's press is a link in its block, not a native view.
                    if node.inline && node.kind == Kind::Text {
                        self.mark_text_block(key);
                    } else {
                        self.mark(key, dirty::EVENTS);
                    }
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
            Op::Command { id, command } => {
                let list = self
                    .key(id)
                    .is_ok_and(|key| self.nodes[key as usize].kind == Kind::List);
                if list
                    && matches!(
                        command,
                        Command::ScrollToIndex { .. } | Command::ScrollToEnd { .. }
                    )
                {
                    if let Ok(key) = self.key(id) {
                        self.mark(key, dirty::LIST);
                    }
                    self.commands.push((id, command));
                } else {
                    self.mount.push(MountItem::Command { id, command });
                }
                Ok(())
            }
        }
    }

    pub(super) fn key(&self, id: NodeId) -> Result<Key, ApplyError> {
        match self.slots.get(id as usize) {
            Some(&k) if k != 0 => Ok(k - 1),
            _ => Err(ApplyError::UnknownNode(id)),
        }
    }

    fn alloc(&mut self, id: NodeId, kind: Kind) -> Result<Key, ApplyError> {
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
        let key = self.nodes.insert(Node::new(id, kind)) as Key;
        self.slots[idx] = key + 1;
        Ok(key)
    }

    fn create(&mut self, id: NodeId, tag: Tag) -> Result<(), ApplyError> {
        let kind = Kind::of(tag);
        let key = self.alloc(id, kind)?;
        let mut layout_style = Style::default().layout;
        style::resolve_normal_alignment(&mut layout_style);
        // The authored style. [`Self::sync_scroll_style`] derives the
        // taffy style (the scroll axis, and a horizontal row).
        let scroll_base = matches!(kind, Kind::Scroll | Kind::List).then(|| layout_style.clone());
        let root_base = matches!(kind, Kind::Screen | Kind::Modal).then(|| layout_style.clone());
        let layout = match kind {
            Kind::Image => self
                .layout
                .new_leaf_with_context(layout_style, LeafContext::Image(None)),
            Kind::Text | Kind::Input => self.layout.new_leaf_with_context(
                layout_style,
                LeafContext::Text(TextContext::new(Arc::default())),
            ),
            Kind::View | Kind::Scroll | Kind::List | Kind::Cell | Kind::Stack => {
                self.layout.new_leaf(layout_style)
            }
            Kind::Screen | Kind::Modal | Kind::HeaderItem => {
                // Layout roots: laid out on their own, not in parent.
                self.layout.new_leaf(layout_style)
            }
            Kind::RawText => unreachable!("raw text is CreateText, not a tag"),
        };
        self.layout.set_tag(layout, key);
        self.nodes[key as usize].layout = Some(layout);
        if kind == Kind::Text {
            self.nodes[key as usize].dirty |= dirty::TEXT_BUILD;
        }
        if let Some(base) = scroll_base {
            self.nodes[key as usize].base_layout = Some(Box::new(base));
        }
        if let Some(base) = root_base {
            self.nodes[key as usize].base_layout = Some(Box::new(base));
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            self.sync_scroll_style(key);
        }
        if kind == Kind::List {
            self.lists.push(key);
        }
        if matches!(kind, Kind::Screen | Kind::Modal | Kind::HeaderItem) {
            self.roots.push(key);
            // Mark dirty to emit initial config.
            if kind == Kind::Screen {
                self.nodes[key as usize].dirty |= dirty::SCREEN;
            } else if kind == Kind::Modal {
                self.nodes[key as usize].dirty |= dirty::MODAL;
            }
        }
        if kind == Kind::Input {
            self.measure_input(key);
        }
        Ok(())
    }

    pub(super) fn insert(
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

        // Inside a block or a span. Other elements inside text are not
        // part of v1; `set_inline` leaves them out of layout.
        let inline = self.nodes[parent as usize].kind == Kind::Text;
        let pos = match before {
            Some(b) => self.nodes[parent as usize]
                .children
                .iter()
                .position(|&c| c == b)
                .expect("anchor is a child"),
            None => self.nodes[parent as usize].children.len(),
        };
        self.set_inline(child, inline);
        self.nodes[child as usize].parent = parent;
        self.nodes[parent as usize].children.insert(pos, child);
        if self.nodes[child as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        self.spacing_changed(parent, child);
        if self.pointer_none_classes > 0 {
            self.pointer_events_moved(parent);
        }
        self.update_direction(child);
        // Style is applied before insert. `env()` was resolved with no
        // parent then; the screen or modal this lands in may owe different
        // insets.
        self.refresh_env(child);
        // A header slot's options are part of the screen's config.
        if self.nodes[child as usize].kind == Kind::HeaderItem
            && self.nodes[parent as usize].kind == Kind::Screen
        {
            self.mark(parent, dirty::SCREEN);
            self.needs_layout = true;
        }
        if inline {
            self.mark_text_block(parent);
            return Ok(());
        }
        // A cell is its list's child natively but a layout root of its own:
        // its size is measured and its frame placed by the list.
        let list_parent = self.nodes[parent as usize].kind == Kind::List;
        if list_parent {
            if self.nodes[child as usize].kind != Kind::Cell {
                log::warn!("a list takes `<cell>` children only");
            }
            if let Some(slot) = self.nodes[child as usize].slot
                && let Some(list) = self.nodes[parent as usize].list.as_mut()
            {
                list.add_slot_cell(slot, child);
                self.mark(parent, dirty::LIST);
            }
            if parent_created {
                self.ensure_created(child);
                let before = self.native_before(parent, child);
                self.mount.push(MountItem::Insert {
                    parent: self.nodes[parent as usize].id,
                    child: self.nodes[child as usize].id,
                    before,
                });
            }
            return Ok(());
        }
        if let Some(child_layout) = self.nodes[child as usize].layout {
            if !self.nodes[child as usize].kind.is_layout_root() {
                let parent_layout = self.nodes[parent as usize]
                    .layout
                    .expect("block parent has layout");
                // Layout roots (a screen's header items) are children of
                // the screen, not of its layout.
                let layout_pos = self.nodes[parent as usize].children[..pos]
                    .iter()
                    .filter(|&&c| !self.nodes[c as usize].kind.is_layout_root())
                    .count();
                self.layout
                    .insert_child_at_index(parent_layout, layout_pos, child_layout);
            }
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
        if self.nodes[key as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        self.spacing_changed(parent, key);
        if self.pointer_none_classes > 0 && self.pointer_region(parent).is_some() {
            self.update_pointer_events(key);
            self.pointer_events_moved(parent);
        }
        let (inline, layout, created, id) = (
            self.nodes[key as usize].inline,
            self.nodes[key as usize].layout,
            self.nodes[key as usize].created,
            self.nodes[key as usize].id,
        );
        if inline {
            self.mark_text_block(parent);
            return;
        }
        if self.nodes[key as usize].kind == Kind::HeaderItem
            && self.nodes[parent as usize].kind == Kind::Screen
        {
            self.mark(parent, dirty::SCREEN);
            self.needs_layout = true;
        }
        if let Some(layout) = layout {
            if self.nodes[parent as usize].kind == Kind::List {
                if self.nodes[key as usize].kind == Kind::Cell
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.remove_cell(key);
                    self.mark(parent, dirty::LIST);
                }
            } else {
                if !self.nodes[key as usize].kind.is_layout_root() {
                    let parent_layout = self.nodes[parent as usize]
                        .layout
                        .expect("block parent has layout");
                    self.layout.remove_child(parent_layout, layout);
                }
                self.needs_layout = true;
            }
            if emit_remove && created && self.nodes[parent as usize].created {
                self.mount.push(MountItem::Remove { id });
            }
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
        if self.nodes[key as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        let node = self.nodes.remove(key as usize);
        if node.class_props.as_ref().and_then(|c| c.pointer_events) == Some(PointerEvents::None) {
            self.pointer_none_classes -= 1;
        }
        self.slots[id as usize] = 0;
        self.lists.retain(|&k| k != key);
        self.roots.retain(|&k| k != key);
        self.attachment_blocks.retain(|&k| k != key);
        self.list_binds.retain(|b| b.list != id);
        for &c in &node.children {
            if let Some(child) = self.nodes.get_mut(c as usize) {
                child.parent = NO_KEY;
            }
        }
        if let Some(layout) = node.layout {
            self.layout.remove(layout);
        }
        if node.created {
            self.mount.push(MountItem::Delete { id });
        }
        Ok(())
    }

    pub(super) fn is_ancestor(&self, ancestor: Key, mut key: Key) -> bool {
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
    pub(super) fn restack(&mut self, key: Key) {
        let node = &self.nodes[key as usize];
        let (parent, id) = (node.parent, node.id);
        if parent == NO_KEY || !node.created || node.inline {
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

    /// Moves `key` (and its subtree) into or out of an inline context.
    fn set_inline(&mut self, key: Key, inline: bool) {
        let node = &mut self.nodes[key as usize];
        if node.inline == inline {
            return;
        }
        node.inline = inline;
        if node.kind != Kind::Text {
            // Other elements inside text are not part of v1; they are ignored.
            return;
        }
        if inline {
            // Block -> span: spans have no layout node or native view.
            if let Some(l) = node.layout.take() {
                self.layout.remove(l);
            }
            if node.created {
                node.created = false;
                self.mount.push(MountItem::Delete { id: node.id });
            }
        } else {
            let mut style = self.nodes[key as usize]
                .base_layout
                .as_deref()
                .cloned()
                .unwrap_or_default();
            if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
                super::nav::add_insets(&mut style, applied[0], applied[1]);
            }
            let cx = LeafContext::Text(TextContext::new(Arc::default()));
            let layout = self.layout.new_leaf_with_context(style, cx);
            self.layout.set_tag(layout, key);
            self.nodes[key as usize].layout = Some(layout);
            self.mark(key, dirty::TEXT_BUILD);
        }
    }

    /// Creates the native views of `key`'s subtree if needed, wiring up the
    /// children in order.
    pub(super) fn ensure_created(&mut self, key: Key) {
        let has_attachments = self.attachment_blocks.contains(&key);
        let pointer = self.mounted_pointer(key);
        let node = &mut self.nodes[key as usize];
        if node.created || node.layout.is_none() {
            return;
        }
        node.created = true;
        node.frame = Bounds::UNSET;
        let id = node.id;
        let kind = node.kind;
        let parent_list =
            kind == Kind::List && node.list.as_ref().is_some_and(|list| list.viewport_parent);
        let mut flags = 0;
        if node.view_style != ViewStyle::default() {
            flags |= dirty::VIEW;
        }
        if !node.animated.is_empty() {
            flags |= dirty::ANIMATION;
        }
        if !node.gestures.is_empty() {
            flags |= dirty::PAN;
        }
        if !node.events.is_empty() || node.long_press_delay.is_some() {
            flags |= dirty::EVENTS;
        }
        if kind == Kind::Text {
            flags |= dirty::TEXT_MOUNT;
        }
        if node.image.as_ref().is_some_and(|source| source.is_some()) {
            flags |= dirty::IMAGE;
        }
        if matches!(kind, Kind::Scroll | Kind::List) && !parent_list {
            // A new native view has not been told its content size.
            node.scroll.as_mut().expect("scroll").content = None;
            flags |= dirty::SCROLL;
        }
        if kind == Kind::Screen {
            flags |= dirty::SCREEN;
        }
        if kind == Kind::Modal {
            flags |= dirty::MODAL;
        }
        if let Some(input) = node.input.as_mut() {
            input.content = None;
            flags |= dirty::INPUT;
            if !input.value.is_empty() {
                flags |= dirty::INPUT_VALUE;
            }
        }
        let slots = node
            .state
            .iter()
            .enumerate()
            .fold(node.state_dirty, |slots, (i, paint)| {
                if paint.is_some() {
                    slots | (1 << i)
                } else {
                    slots
                }
            });
        if slots != 0 {
            node.state_dirty = slots;
            flags |= dirty::STATE;
        }
        if node.test_id.is_some() {
            flags |= dirty::TEST_ID;
        }
        if node.selectable {
            flags |= dirty::SELECTABLE;
        }
        if pointer != PointerEvents::Auto {
            flags |= dirty::POINTER;
        }
        if kind == Kind::Text && has_attachments {
            // Its attachments' views go inside the new view.
            node.attached.clear();
            flags |= dirty::TEXT_BUILD;
        }
        if !node.accessibility.is_default() {
            flags |= dirty::ACCESSIBILITY;
        }
        self.mount.push(MountItem::Create {
            id,
            kind: if parent_list {
                ViewKind::View
            } else {
                kind.view_kind()
            },
        });
        if let Some(item) = self.creation_tracking(key) {
            self.mount.push(item);
        }
        self.mark(key, flags);
        if !kind.is_container() {
            return;
        }
        let mut children = mem::take(&mut self.nodes[key as usize].children);
        let original = children.clone();
        if children
            .iter()
            .any(|&c| self.nodes[c as usize].z_index != 0)
        {
            children.sort_by_key(|&c| self.nodes[c as usize].z_index);
        }
        for &child in &children {
            if self.nodes[child as usize].layout.is_none() {
                continue;
            }
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
}
