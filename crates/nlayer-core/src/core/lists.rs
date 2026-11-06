//! Virtualized list windows on the shadow tree.

use super::*;

/// The layout of a parent-viewport list's spacer: as long as the list's
/// content along the main axis.
fn spacer_style(len: f32) -> taffy::Style {
    taffy::Style {
        size: taffy::Size {
            width: taffy::Dimension::auto(),
            height: taffy::Dimension::length(len.max(0.0)),
        },
        flex_shrink: 0.0,
        ..Default::default()
    }
}

impl<M: TextEngine> Core<M> {
    /// `viewport` of list `key`: `Parent` virtualizes it against the scroll
    /// view around it and gives that view the list's full length. The mode
    /// is fixed when the list's native view is made; a later change is
    /// ignored, as in nsh.
    pub(super) fn set_list_viewport(&mut self, key: Key, parent: bool) {
        let Some(list) = self.nodes[key as usize].list.as_ref() else {
            return;
        };
        if list.viewport_parent == parent {
            return;
        }
        if self.nodes[key as usize].created {
            log::warn!(
                "List {}: `viewport` is fixed when the list is made; the change is ignored (render a new list)",
                self.nodes[key as usize].id
            );
            return;
        }
        self.nodes[key as usize]
            .list
            .as_mut()
            .expect("list")
            .viewport_parent = parent;
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        if parent {
            let len = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .content_len();
            let spacer = self.layout.new_leaf(spacer_style(len));
            self.layout.add_child(layout, spacer);
            self.nodes[key as usize].list.as_mut().expect("list").spacer = Some((spacer, len));
        } else if let Some((spacer, _)) = self.nodes[key as usize]
            .list
            .as_mut()
            .expect("list")
            .spacer
            .take()
        {
            self.layout.remove(spacer);
        }
        self.sync_scroll_style(key);
        self.needs_layout = true;
        self.mark(key, dirty::LIST);
    }

    /// Gives a parent-viewport list's spacer the new content length: the
    /// scroll view around it scrolls over the whole list.
    fn sync_list_spacer(&mut self, key: Key) {
        let Some(list) = self.nodes[key as usize].list.as_ref() else {
            return;
        };
        let Some((spacer, was)) = list.spacer else {
            return;
        };
        let len = list.content_len();
        if len == was {
            return;
        }
        self.nodes[key as usize].list.as_mut().expect("list").spacer = Some((spacer, len));
        self.layout.set_style(spacer, spacer_style(len));
        self.needs_layout = true;
        self.spacer_changed = true;
    }

    /// The offset the scroll view around `key` is at, and its axis.
    fn outer_scroll_offset(&self, key: Key) -> Option<(Point<Points>, bool)> {
        let mut at = self.nodes[key as usize].parent;
        while at != NO_KEY {
            let node = &self.nodes[at as usize];
            if node.kind == Kind::Scroll || node.kind == Kind::List {
                let scroll = node.scroll.as_ref()?;
                return Some((scroll.offset, scroll.horizontal));
            }
            at = node.parent;
        }
        None
    }

    /// Moves the cells the runtime must render before the frame shows.
    pub fn take_list_binds(&mut self, out: &mut Vec<ListBind>) {
        out.append(&mut self.list_binds);
    }

    /// Whether lists have overscan cells waiting to bind.
    pub fn has_paced_list_binds(&self) -> bool {
        !self.paced_lists.is_empty()
    }

    /// Hands out up to `cells` of the overscan cells waiting to bind, one
    /// per list in turn (a list is not starved by the one before it), each
    /// list's nearest its leading edge first. What a list assigns now is
    /// what waits: cells it left or whose items went are never handed out.
    pub fn take_paced_list_binds(&mut self, cells: usize, out: &mut Vec<ListBind>) {
        let mut left = cells;
        let mut lists = std::mem::take(&mut self.paced_lists);
        let mut one = std::mem::take(&mut self.paced_scratch);
        let start = out.len();
        while left > 0 && !lists.is_empty() {
            lists.retain(|&id| {
                if left == 0 {
                    return true;
                }
                let Some(list) = self
                    .key(id)
                    .ok()
                    .and_then(|key| self.nodes[key as usize].list.as_mut())
                else {
                    return false;
                };
                one.clear();
                left -= list.take_paced(1, &mut one);
                if let Some((slot, index)) = one.pop() {
                    match out[start..].iter_mut().find(|b| b.list == id) {
                        Some(bind) => bind.binds.push((slot, index)),
                        None => out.push(ListBind {
                            list: id,
                            binds: vec![(slot, index)],
                        }),
                    }
                }
                list.has_paced()
            });
        }
        self.paced_lists = lists;
        self.paced_scratch = one;
    }

    /// Gets the viewable item range for a list (for ViewableItemsChange event payload).
    pub fn viewable_range(&self, id: NodeId) -> Option<(u32, u32)> {
        let key = self.key(id).ok()?;
        self.nodes[key as usize].list.as_ref()?.viewable_range
    }

    /// Positions the rows of every list: measures the cells that show
    /// their item, follows a scroll command's target and asks the runtime
    /// for the rows the window needs.
    pub(super) fn update_lists(&mut self) {
        for i in 0..self.lists.len() {
            let key = self.lists[i];
            if !self.nodes[key as usize].created {
                continue;
            }
            let viewport = if self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent
            {
                // Find parent scroll view and use its viewport
                let mut parent_key = self.nodes[key as usize].parent;
                let mut parent_viewport = 0.0;

                while parent_key != NO_KEY {
                    if self.nodes[parent_key as usize].kind == Kind::Scroll {
                        parent_viewport = self.nodes[parent_key as usize]
                            .layout
                            .map(|layout| self.rounded_layout(layout).size.height)
                            .unwrap_or(0.0);
                        break;
                    }
                    parent_key = self.nodes[parent_key as usize].parent;
                }

                parent_viewport
            } else {
                self.nodes[key as usize]
                    .layout
                    .map(|layout| self.rounded_layout(layout).size.height)
                    .unwrap_or(0.0)
            };

            let cross = self.nodes[key as usize]
                .layout
                .map(|layout| self.rounded_layout(layout).size.width)
                .unwrap_or(0.0);

            self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .viewport = viewport;

            // Sizes of cells showing their item, and of the extras.
            let (mut ready, extras) = {
                let list = self.nodes[key as usize].list.as_ref().expect("list");
                (
                    list.slots
                        .iter()
                        .filter_map(|s| s.ready().map(|i| (i, s.cell)))
                        .collect::<Vec<_>>(),
                    list.extras,
                )
            };
            let mut anchor = 0.0;
            let mut scrolls: Vec<(f32, bool)> = Vec::new();
            for &(index, cell) in &ready {
                if let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) {
                    let size = self.rounded_layout(cell_layout).size.height;
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    anchor += list.measured(index, size);
                }
            }
            for (kind, extra) in [
                list::ExtraKind::Header,
                list::ExtraKind::Footer,
                list::ExtraKind::Empty,
            ]
            .into_iter()
            .zip(extras)
            {
                if let Some(cell_layout) =
                    self.nodes.get(extra.cell as usize).and_then(|c| c.layout)
                {
                    let size = self.rounded_layout(cell_layout).size.height;
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    anchor += list.measured_extra(kind, size);
                }
            }
            {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.offset += anchor;
                anchor += list.take_splice_anchor();
                // Inverted lists start at the end and keep that distance as
                // content or the viewport changes. Item 0 stays in view.
                anchor += list.anchor_end();
                if let Some(target) = list.follow_pin(anchor) {
                    scrolls.push((target, true));
                }
            }

            // Scroll commands for the list, now that sizes are known.
            // A command replaces a pin retarget from this same frame:
            // both would be `ScrollTo`s, and the earlier one would win
            // on the platform after the batch.
            let id = self.nodes[key as usize].id;
            let mut commands = Vec::new();
            for command in &self.commands {
                if command.0 == id {
                    commands.push(command.1.clone());
                }
            }
            self.commands.retain(|c| c.0 != id);
            if !commands.is_empty() {
                scrolls.clear();
            }
            for command in commands {
                match command {
                    Command::ScrollToIndex { index, animated } => {
                        let target = list::PinTarget::Index(index);
                        let list = self.nodes[key as usize].list.as_mut().expect("list");
                        let offset = list.target_offset(target);
                        // The window jumps only when the platform moves
                        // now. An animation stays on the reported offset:
                        // jumping the cells to the destination while the
                        // scroll view is still showing the old rect blanks
                        // a long list (nsh does the same).
                        list.pin(Some(target), offset, animated);
                        if !animated {
                            list.jump(offset);
                        }
                        scrolls.push((offset, animated));
                    }
                    Command::ScrollToEnd { animated } => {
                        // The core owns the content length: the end is an
                        // offset it can name, and it follows it as rows
                        // measure, like a pinned index.
                        let target = list::PinTarget::End;
                        let list = self.nodes[key as usize].list.as_mut().expect("list");
                        let offset = list.target_offset(target);
                        list.pin(Some(target), offset, animated);
                        if !animated {
                            list.jump(offset);
                        }
                        scrolls.push((offset, animated));
                    }
                    _ => {}
                }
            }

            // Content can shrink below the offset (the last page cleared):
            // follow the platform's clamp before placing the window.
            let content_len = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .content_len();
            let parent_mode = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent;
            if parent_mode {
                // The page around it learns the list's length, and the
                // list follows where that page is scrolled to.
                self.sync_list_spacer(key);
                if let Some((offset, horizontal)) = self.outer_scroll_offset(key) {
                    self.update_viewport_parent_list_offset(key, offset, horizontal);
                }
            } else {
                let max_offset = (content_len - viewport).max(0.0);
                if self.nodes[key as usize].list.as_ref().expect("list").offset > max_offset {
                    let animating = self.nodes[key as usize]
                        .list
                        .as_ref()
                        .expect("list")
                        .pin_animating();
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    list.jump(max_offset);
                    // An in-flight `scrollTo` already names its end. Another
                    // jump here cancels that animation and stops short.
                    if !animating {
                        scrolls.push((max_offset, false));
                    }
                }
            }

            let mut binds = Vec::new();
            let load_more = {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.update_window(&mut binds)
            };
            if load_more {
                self.events.push((id, EventKind::LoadMore));
            }
            // Check viewable items range
            if self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .check_viewable_items()
                .is_some()
            {
                self.events.push((id, EventKind::ViewableItemsChange));
            }
            if !binds.is_empty() {
                self.list_binds.push(ListBind { list: id, binds });
            }
            {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                if list.paced && list.has_paced() && !self.paced_lists.contains(&id) {
                    self.paced_lists.push(id);
                }
            }

            ready.clear();
            ready.extend(
                self.nodes[key as usize]
                    .list
                    .as_ref()
                    .expect("list")
                    .slots
                    .iter()
                    .filter_map(|s| s.ready().map(|i| (i, s.cell))),
            );

            let (window_start, window_end) = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .render_window();
            let hidden = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .hidden_y();
            let items_changed = self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .items_changed;
            // Cells without an item can still show their last one: move
            // those whose old place is in the window out of sight.
            let unbound: Vec<Key> = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .slots
                .iter()
                .filter(|s| s.ready().is_none() && s.cell != u32::MAX)
                .map(|s| s.cell)
                .collect();
            for cell in unbound {
                let frame = self.nodes[cell as usize].frame;
                if frame.size.width.is_nan() {
                    continue;
                }
                let (start, size) = (frame.origin.y.0, frame.size.height.0);
                if (items_changed || (start + size > window_start && start < window_end))
                    && frame.origin.y.0 != hidden
                {
                    let away = Bounds {
                        origin: Point::new(frame.origin.x, Points(hidden)),
                        ..frame
                    };
                    self.set_frame(cell, away);
                }
            }
            // Place the cells that show their item.
            for &(index, cell) in &ready {
                let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) else {
                    continue;
                };
                let size = self.rounded_layout(cell_layout).size;
                let (column, width) = {
                    let list = self.nodes[key as usize].list.as_ref().expect("list");
                    list.column(cross, list.column_of(index))
                };
                let at = self.nodes[key as usize]
                    .list
                    .as_ref()
                    .expect("list")
                    .item_at(index);

                let (adjusted_at, sticky) = {
                    let list = self.nodes[key as usize].list.as_ref().expect("list");
                    let base_at = list.to_content(at, size.height);
                    let sticky =
                        list.sticky_enabled() && list.sticky_indices.binary_search(&index).is_ok();
                    (base_at + list.sticky_shift(index, size.height), sticky)
                };

                let frame = rect(column, adjusted_at, width, size.height);
                self.set_frame(cell, frame);
                self.set_cell_sticky(cell, sticky);
                if std::mem::take(&mut self.nodes[cell as usize].relaid) {
                    let children = std::mem::take(&mut self.nodes[cell as usize].children);
                    for &child in &children {
                        self.emit_frames(child);
                    }
                    self.nodes[cell as usize].children = children;
                }
            }
            // Header, footer and the empty placeholder.
            let (blocks, count) = {
                let list = self.nodes[key as usize].list.as_ref().expect("list");
                (list.blocks(), list.count)
            };
            let (full_x, full_width) = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .full_width(cross);
            let ats = [blocks.header, blocks.footer, blocks.body];
            for (i, extra) in extras.iter().enumerate() {
                if extra.cell == u32::MAX {
                    continue;
                }
                let cell = extra.cell;
                if i == 2 && count > 0 {
                    // The empty placeholder shows only without items.
                    let frame = self.nodes[cell as usize].frame;
                    if !frame.size.width.is_nan() && frame.origin.y.0 != hidden {
                        let away = Bounds {
                            origin: Point::new(frame.origin.x, Points(hidden)),
                            ..frame
                        };
                        self.set_frame(cell, away);
                    }
                    continue;
                }
                if let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) {
                    let size = self.rounded_layout(cell_layout).size;
                    let at = self.nodes[key as usize]
                        .list
                        .as_ref()
                        .expect("list")
                        .to_content(ats[i], size.height);
                    let frame = rect(full_x, at, full_width, size.height);
                    self.set_frame(cell, frame);
                    if std::mem::take(&mut self.nodes[cell as usize].relaid) {
                        let children = std::mem::take(&mut self.nodes[cell as usize].children);
                        for &child in &children {
                            self.emit_frames(child);
                        }
                        self.nodes[cell as usize].children = children;
                    }
                }
            }
            self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .items_changed = false;

            let content = Size {
                width: Points(cross),
                height: Points(content_len),
            };
            let old_len = self.nodes[key as usize]
                .scroll
                .as_ref()
                .and_then(|s| s.content)
                .map_or(0.0, |c| c.height.0);
            let viewport_parent = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent;

            if !viewport_parent {
                // Only set scroll content if list has its own scroll view
                if anchor != 0.0 {
                    if content_len >= old_len {
                        self.set_scroll_content(key, content);
                        self.mount.push(MountItem::ScrollBy {
                            id,
                            dx: Points::ZERO,
                            dy: Points(anchor),
                        });
                    } else {
                        self.mount.push(MountItem::ScrollBy {
                            id,
                            dx: Points::ZERO,
                            dy: Points(anchor),
                        });
                        self.set_scroll_content(key, content);
                    }
                    self.nodes[key as usize]
                        .list
                        .as_mut()
                        .expect("list")
                        .issue_scroll_by(anchor);
                } else {
                    self.set_scroll_content(key, content);
                }
            }
            // The content is final before a scroll command: platforms
            // clamp an offset to the content they know.
            for (offset, animated) in scrolls {
                self.nodes[key as usize]
                    .list
                    .as_mut()
                    .expect("list")
                    .issue_scroll_to(offset);
                self.mount.push(MountItem::Command {
                    id,
                    command: Command::ScrollTo {
                        x: Points::ZERO,
                        y: Points(offset),
                        animated,
                    },
                });
            }
        }
    }
}
