//! Scroll offset, content size and pull to refresh.

use super::*;

impl<M: TextEngine> Core<M> {
    /// The platform scrolled a list to `offset`. A report from before a
    /// scroll the core issued is replayed onto it.
    pub fn set_scroll_offset(&mut self, id: NodeId, offset: Point<Points>) {
        let Ok(key) = self.key(id) else { return };

        // Update list's own offset if this is a list node
        if let Some(list) = self.nodes[key as usize].list.as_mut() {
            list.reported(offset.y.0);
            self.mark(key, dirty::LIST);
        }

        // If this is a scroll node, recompute list offsets, sticky, and viewport_parent
        if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
            scroll.offset = offset;
            self.recompute_scroll_lists(key, offset);
        }
    }

    /// The platform scrolled a scroll view to `offset`. Checks for pull-to-refresh
    /// and updates pull distance animation value.
    pub fn update_scroll_position(&mut self, id: NodeId, offset: Point<Points>) {
        let Ok(key) = self.key(id) else { return };
        let Some(scroll) = self.nodes[key as usize].scroll.as_mut() else {
            return;
        };
        scroll.offset = offset;

        // Calculate pull distance (negative offset means pulling down)
        let pull_distance = if offset.y.0 < 0.0 { -offset.y.0 } else { 0.0 };

        // Update pullValue animation node
        if let Some(pull_node) = scroll.pull_value_node {
            let value = if scroll.refreshing {
                // When refreshing, clamp to indicator height
                scroll.refresh_indicator.map(|h| h.0).unwrap_or(0.0)
            } else {
                pull_distance
            };
            self.mount
                .push(MountItem::Animation(crate::animation::AnimationOp::Set {
                    node: pull_node,
                    value,
                }));
        }

        // Check for pull-to-refresh trigger
        if let Some(indicator_height) = scroll.refresh_indicator
            && pull_distance >= indicator_height.0
        {
            // Fire refresh event only once per cycle
            if !scroll.refresh_reported && !scroll.refreshing {
                scroll.refresh_reported = true;
                self.events.push((id, EventKind::Refresh));
            }
        }

        // Recompute list offsets, sticky positions, and viewport_parent children
        self.recompute_scroll_lists(key, offset);
    }

    /// Recompute list offsets, sticky positions, and windowing when a scroll view moves.
    /// Used by both `set_scroll_offset` (platform events) and `update_scroll_position` (messages).
    fn recompute_scroll_lists(&mut self, key: Key, offset: Point<Points>) {
        let Some(scroll) = self.nodes[key as usize].scroll.as_ref() else {
            return;
        };
        let horizontal = scroll.horizontal;

        // Update list offset if this scroll view has a list (for scroll+list nodes)
        if let Some(list) = self.nodes[key as usize].list.as_mut() {
            let new_offset = if horizontal { offset.x.0 } else { offset.y.0 };
            list.note_platform_offset(new_offset);
            if list.dirty {
                self.mark(key, dirty::LIST);
            }
        }

        // Update child lists that use viewport="parent"
        let children = self.nodes[key as usize].children.clone();
        for &child_key in &children {
            self.update_viewport_parent_list_offset(child_key, offset, horizontal);
        }
    }

    pub(super) fn update_viewport_parent_list_offset(
        &mut self,
        key: Key,
        offset: Point<Points>,
        horizontal: bool,
    ) {
        // Check if this node has a viewport_parent list
        let is_viewport_parent = self.nodes[key as usize]
            .list
            .as_ref()
            .is_some_and(|l| l.viewport_parent);

        if is_viewport_parent {
            // Calculate cumulative position from list up to parent scroll
            let mut cumulative_pos = 0.0;
            let mut current_key = key;

            // Walk up the parent chain until we find the scroll node
            loop {
                // Add this node's layout position; the list's own border
                // and padding come before its content.
                if let Some(layout) = self.nodes[current_key as usize].layout {
                    let l = self.rounded_layout(layout);
                    cumulative_pos += if horizontal {
                        l.location.x
                    } else {
                        l.location.y
                    };
                    if current_key == key {
                        cumulative_pos += if horizontal {
                            l.border.left + l.padding.left
                        } else {
                            l.border.top + l.padding.top
                        };
                    }
                }

                // Move to parent
                let parent_key = self.nodes[current_key as usize].parent;
                if parent_key == NO_KEY {
                    break;
                }

                // Stop if parent is a scroll node (this is our viewport parent)
                if self.nodes[parent_key as usize].kind == Kind::Scroll {
                    break;
                }

                current_key = parent_key;
            }

            // List offset = parent scroll offset - list's cumulative position in scroll content
            let parent_offset = if horizontal { offset.x.0 } else { offset.y.0 };
            let new_offset = parent_offset - cumulative_pos;

            if let Some(list) = self.nodes[key as usize].list.as_mut() {
                list.note_platform_offset(new_offset);
                if list.dirty {
                    self.mark(key, dirty::LIST);
                }
            }
        }

        // Recursively check children
        let children = self.nodes[key as usize].children.clone();
        for &child_key in &children {
            self.update_viewport_parent_list_offset(child_key, offset, horizontal);
        }
    }

    /// The taffy style of a scroll view: children overflow on its axis, and a
    /// horizontal one lays them out in a row. Right to left, that row is laid
    /// out left to right (padding and border swapped back) and
    /// [`Core::emit_frames_in`] mirrors the children: a right-to-left row would
    /// overflow to negative offsets.
    pub(super) fn sync_scroll_style(&mut self, key: Key) {
        let (layout, mut style, horizontal, rtl) = {
            let node = &self.nodes[key as usize];
            let (Some(layout), Some(base)) = (node.layout, node.base_layout.as_deref()) else {
                return;
            };
            let horizontal = node.scroll.as_ref().is_some_and(|scroll| scroll.horizontal);
            (layout, base.clone(), horizontal, node.rtl)
        };
        // On the physical edges, then swapped with the padding when a
        // right-to-left row is laid out left to right.
        if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
            super::nav::add_insets(&mut style, applied[0], applied[1]);
        }
        if self.nodes[key as usize].kind == Kind::List
            && self.nodes[key as usize]
                .list
                .as_ref()
                .is_some_and(|list| list.viewport_parent)
        {
            style.overflow = taffy::Point {
                x: taffy::Overflow::Hidden,
                y: taffy::Overflow::Hidden,
            };
            self.layout.set_style(layout, style);
            self.needs_layout = true;
            return;
        }
        if horizontal {
            style.flex_direction = taffy::FlexDirection::Row;
            style.overflow.x = taffy::Overflow::Scroll;
            if rtl {
                mem::swap(&mut style.padding.left, &mut style.padding.right);
                mem::swap(&mut style.border.left, &mut style.border.right);
                style.direction = taffy::Direction::Ltr;
            }
        } else {
            style.overflow.y = taffy::Overflow::Scroll;
        }
        self.layout.set_style(layout, style);
        self.needs_layout = true;
    }

    pub(super) fn set_scroll_content(&mut self, key: Key, size: Size<Points>) {
        self.write_scroll_content(key, size, false);
    }

    /// Sends the current content again so snap, paging and deceleration
    /// reach the platform when the size did not change.
    pub(super) fn refresh_scroll_content(&mut self, key: Key) {
        let Some(size) = self.nodes[key as usize]
            .scroll
            .as_ref()
            .and_then(|scroll| scroll.content)
        else {
            return;
        };
        self.write_scroll_content(key, size, true);
    }

    fn write_scroll_content(&mut self, key: Key, size: Size<Points>, force: bool) {
        let (
            id,
            rtl,
            kind,
            viewport,
            horizontal,
            old,
            content_item,
            refresh_indicator,
            paging,
            snap_interval,
            snap_offsets,
            snap_alignment,
            disable_interval_momentum,
            deceleration,
        ) = {
            let node = &mut self.nodes[key as usize];
            let Some(scroll) = node.scroll.as_mut() else {
                return;
            };
            if !force && scroll.content == Some(size) {
                return;
            }
            let old = scroll.content.replace(size);
            (
                node.id,
                node.rtl,
                node.kind,
                node.frame.size.width,
                scroll.horizontal,
                old,
                scroll.content_item,
                scroll.refresh_indicator,
                scroll.paging,
                scroll.snap_interval,
                scroll.snap_offsets.clone(),
                scroll.snap_alignment,
                scroll.disable_interval_momentum,
                scroll.deceleration,
            )
        };
        // Items are only inserted before it since (a `ScrollBy`), so it is
        // at its index or later.
        let queued = content_item.and_then(|i| {
            let at = self.mount.get(i..)?.iter().position(
                |item| matches!(item, MountItem::ScrollContent { id: queued, .. } if *queued == id),
            )?;
            Some(i + at)
        });
        if let Some(j) = queued {
            self.mount[j] = MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets: snap_offsets.clone(),
                snap_alignment: snap_alignment as u8,
                disable_interval_momentum,
                deceleration,
            };
        }
        // A scroll queued after it must still come before the new size.
        let scrolled_since = queued.is_some_and(|j| {
            self.mount[j + 1..].iter().any(
                |item| matches!(item, MountItem::ScrollBy { id: scrolled, .. } if *scrolled == id),
            )
        });
        if queued.is_none() || scrolled_since {
            let at = self.mount.len();
            self.mount.push(MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets,
                snap_alignment: snap_alignment as u8,
                disable_interval_momentum,
                deceleration,
            });
            if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
                scroll.content_item = Some(at);
            }
        }
        // Right to left, a horizontal scroll view starts at its right end
        // and keeps its distance from it as the content changes.
        if kind == Kind::Scroll && horizontal && rtl {
            match old {
                None => self.mount.push(MountItem::Command {
                    id,
                    command: Command::ScrollTo {
                        x: (size.width - viewport).max(Points::ZERO),
                        y: Points::ZERO,
                        animated: false,
                    },
                }),
                Some(old) if old.width != size.width => {
                    self.mount.push(MountItem::ScrollBy {
                        id,
                        dx: size.width - old.width,
                        dy: Points::ZERO,
                    });
                }
                Some(_) => {}
            }
        }
    }
}
