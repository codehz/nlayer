//! Dirty flush and the mount batch for one commit.

use super::*;

impl<M: TextEngine> Core<M> {
    /// The events the core raised for JS.
    pub fn take_events(&mut self, out: &mut Vec<(NodeId, EventKind)>) {
        out.append(&mut self.events);
    }

    /// Finishes a turn: runs layout and returns the mount items, or `None`
    /// when nothing changed.
    pub fn commit(&mut self) -> Option<MountBatch> {
        if self.focus_moved {
            self.update_focus_within();
        }
        self.flush_dirty();
        self.layout_round();
        self.update_lists();
        // A parent-viewport list gives the page its content length after
        // measuring it: lay the page out again with that length, until it
        // settles.
        for _ in 0..4 {
            if !mem::take(&mut self.spacer_changed) {
                break;
            }
            self.needs_layout = true;
            self.layout_round();
            self.update_lists();
        }
        self.flush_text();
        if self.mount.is_empty() {
            None
        } else {
            Some(MountBatch {
                items: mem::take(&mut self.mount),
            })
        }
    }

    /// Sticky list cells paint above the rows scrolling under them. Only
    /// the band this sets (`1`) is touched, so a style `zIndex` stays.
    pub(super) fn set_cell_sticky(&mut self, cell: Key, sticky: bool) {
        const STICKY_Z: i32 = 1;
        let node = &mut self.nodes[cell as usize];
        let want = if sticky { STICKY_Z } else { 0 };
        if node.z_index == want || (!sticky && node.z_index != STICKY_Z) {
            return;
        }
        if sticky && node.z_index != 0 && node.z_index != STICKY_Z {
            return;
        }
        node.z_index = want;
        self.restack(cell);
    }

    pub(super) fn mark(&mut self, key: Key, flags: u32) {
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
        loop {
            self.flush_dirty_once();
            // Text blocks create the views of new attachments while
            // flushing, which queues them; texts inside attachments built
            // after their block resize them, which builds the block again.
            self.check_attachments();
            if self.queue.is_empty() {
                break;
            }
        }
    }

    fn flush_dirty_once(&mut self) {
        let queue = mem::take(&mut self.queue);
        for key in queue {
            let (mut flags, has_layout) = {
                let Some(node) = self.nodes.get_mut(key as usize) else {
                    continue;
                };
                let flags = mem::take(&mut node.dirty);
                node.queued = false;
                (flags, node.layout.is_some())
            };
            if flags & dirty::TEXT_BUILD != 0 && has_layout && self.rebuild_text(key) {
                flags |= dirty::TEXT_MOUNT;
            }
            let node = &self.nodes[key as usize];
            if !node.created {
                continue;
            }
            let id = node.id;
            if flags & dirty::VIEW != 0 {
                let style = self.mounted_view_style(key);
                self.mount.push(MountItem::ViewStyle { id, style });
            }
            if flags & dirty::ANIMATION != 0 {
                let bindings = self.nodes[key as usize].animated.clone();
                self.mount
                    .push(MountItem::AnimationBindings { id, bindings });
            }
            if flags & dirty::PAN != 0 {
                let gestures = self.nodes[key as usize].gestures;
                self.mount.push(MountItem::PanBinding {
                    id,
                    x: gestures.pan_x,
                    y: gestures.pan_y,
                });
                self.mount.push(MountItem::PinchBinding {
                    id,
                    scale: gestures.pinch,
                });
                self.mount.push(MountItem::RotationBinding {
                    id,
                    angle: gestures.rotation,
                });
            }
            if flags & dirty::EVENTS != 0 {
                let events = self.nodes[key as usize].events;
                self.mount.push(MountItem::Events { id, events });
                let delay = self.nodes[key as usize].long_press_delay;
                self.mount.push(MountItem::LongPressDelay { id, delay });
            }
            if flags & dirty::SELECTABLE != 0 {
                let selectable = self.nodes[key as usize].selectable;
                self.mount.push(MountItem::Selectable { id, selectable });
            }
            if flags & dirty::POINTER != 0 {
                let events = self.mounted_pointer(key);
                self.mount.push(MountItem::PointerEvents { id, events });
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
            if flags & dirty::SCROLL != 0
                && let Some(scroll) = self.nodes[key as usize].scroll.as_ref()
            {
                self.mount.push(MountItem::ScrollConfig {
                    id,
                    horizontal: scroll.horizontal,
                    taps: scroll.taps,
                    dismiss: scroll.dismiss,
                });
                // Snap, paging and deceleration ride on ScrollContent. A
                // size-only send drops a later toggle (the gallery's
                // snapToInterval switch).
                if scroll.content.is_some() {
                    self.refresh_scroll_content(key);
                }
            }
            if flags & dirty::TEXT_MOUNT != 0 {
                self.queue_text(key);
            }
            if flags & (dirty::INPUT | dirty::INPUT_VALUE) != 0 {
                self.emit_input(key, id, flags);
            }
            if flags & dirty::REFRESHING != 0
                && let Some(scroll) = self.nodes[key as usize].scroll.as_ref()
            {
                self.mount.push(MountItem::Refreshing {
                    id,
                    refreshing: scroll.refreshing,
                });
            }
            if flags & dirty::SELECTION != 0 {
                let selection = self.nodes[key as usize]
                    .input
                    .as_mut()
                    .and_then(|input| input.selection.take());
                if let Some((start, end)) = selection {
                    self.mount.push(MountItem::Command {
                        id,
                        command: Command::SetSelection { start, end },
                    });
                }
            }
            if flags & dirty::STATE != 0 {
                let slots = mem::take(&mut self.nodes[key as usize].state_dirty);
                for slot in 0..3 {
                    if slots & (1 << slot) != 0 {
                        self.emit_state(key, id, slot);
                    }
                }
            }
            if flags & dirty::TEST_ID != 0 {
                let test_id = self.nodes[key as usize].test_id.clone();
                self.mount.push(MountItem::TestId { id, test_id });
            }
            if flags & dirty::ACCESSIBILITY != 0 {
                let accessibility = Box::new(self.nodes[key as usize].accessibility.clone());
                self.mount
                    .push(MountItem::Accessibility { id, accessibility });
            }
            if flags & dirty::SCREEN != 0
                && let Some(config) = self.screen_config(key)
            {
                // Before the layout that follows, with the header height it
                // measured last (layout_detached emits again if it moves).
                self.mount.push(MountItem::ScreenConfig {
                    id,
                    config: Box::new(config),
                });
            }
            if flags & dirty::MODAL != 0
                && let Some(root) = self.nodes[key as usize].root.as_ref()
            {
                let config = root.modal;
                self.mount.push(MountItem::ModalConfig { id, config });
            }
        }
    }

    /// The view style as platforms take it: translations by a fraction of
    /// the size resolved with the current frame.
    pub(super) fn mounted_view_style(&self, key: Key) -> Box<ViewStyle> {
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

    /// One pointer state's paint, with fractional translations resolved
    /// against the current frame. A default style clears the state.
    pub(super) fn emit_state(&mut self, key: Key, id: NodeId, slot: usize) {
        let node = &self.nodes[key as usize];
        let (view, text) = match &node.state[slot] {
            Some(paint) => {
                let mut view = paint.view.clone();
                if view.transform.has_fractions() {
                    let size = if node.frame.size.width.is_nan() {
                        Size::default()
                    } else {
                        node.frame.size
                    };
                    view.transform = view.transform.resolve_fractions(size);
                }
                (view, paint.text)
            }
            None => (ViewStyle::default(), TextStyle::default()),
        };
        self.mount.push(MountItem::StateStyle {
            id,
            interaction: Interaction::from_index(slot),
            view: Box::new(view),
            text,
        });
    }
}
