//! Mount batch apply, animation ticks and per-item updates.

use nlayer_core::geometry::Points;
use nlayer_core::style::{TextStyle, ViewStyle};
use nlayer_core::{EventKind, MountBatch, MountItem, NodeId, ROOT_ID};
use objc2::{DefinedClass, Message};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};

use super::*;

impl Mounter {
    pub(crate) fn apply(&mut self, batch: MountBatch) {
        // One transaction for the batch. nsh wraps each decoration the same
        // way: a direct layer edit otherwise fades in over a quarter second.
        // An animated `scrollTo` starts its own animation, so it runs after.
        let mut animated = Vec::new();
        quiet(|| {
            for item in batch.items {
                if let MountItem::Command {
                    id,
                    command:
                        nlayer_core::Command::ScrollTo {
                            x,
                            y,
                            animated: true,
                        },
                } = item
                {
                    animated.push((id, x, y));
                    continue;
                }
                self.apply_item(item);
            }
        });
        for (id, x, y) in animated {
            let Some(Native::Scroll(scroll)) = self.native(id) else {
                continue;
            };
            scroll.setContentOffset_animated(clamped_scroll_offset(scroll, x.0, y.0), true);
        }
        self.settle_content_sizes();
        self.finish_navigation();
        self.pass_modal_presses();
        self.apply_animations();
        self.report_finished();
        for (_, area) in &self.areas {
            area.batch_applied();
        }
    }

    /// Runs one display frame: the graph moves, bound values are painted,
    /// and finished animations go back to the engine.
    pub(super) fn tick_animation(&mut self, now: f64) {
        if self.frame_pending {
            self.frame_pending = false;
            if let Some(control) = &self.control {
                control.frame();
            }
        }
        if self.animator.tick(now) {
            self.apply_animations();
        }
        self.report_finished();
        self.update_link();
    }

    pub(super) fn update_link(&mut self) {
        let animating = self.animator.animating();
        if let Some(link) = self.relay.ivars().link.borrow().as_ref() {
            link.setPaused(!(animating || self.frame_pending));
        }
    }

    pub(super) fn report_finished(&mut self) {
        let finished = self.animator.take_finished();
        if !finished.is_empty()
            && let Some(control) = &self.control
        {
            control.animations_finished(finished);
        }
    }

    /// Paints every bound property over the slot's static style.
    pub(super) fn apply_animations(&mut self) {
        let ids: Vec<NodeId> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.as_ref()
                    .is_some_and(|slot| !slot.bindings.is_empty())
                    .then_some(i as NodeId)
            })
            .collect();
        for id in ids {
            let Some((view, mut style, bindings, scroll)) = self.slot(id).map(|slot| {
                (
                    slot.native.view().retain(),
                    slot.style.clone(),
                    slot.bindings.clone(),
                    matches!(slot.native, Native::Scroll(_)),
                )
            }) else {
                continue;
            };
            for (prop, node) in bindings {
                let value = self.animator.eval(node);
                match prop {
                    nlayer_core::animation::AnimationProp::Opacity => {
                        style.opacity = value.number()
                    }
                    nlayer_core::animation::AnimationProp::BackgroundColor => {
                        style.background_color = nlayer_core::style::Color(value.color())
                    }
                    nlayer_core::animation::AnimationProp::BorderColor => {
                        style.borders.colors = [nlayer_core::style::Color(value.color()); 4]
                    }
                    nlayer_core::animation::AnimationProp::Transform(i) => {
                        if style.transform.ops().get(i as usize).is_some() {
                            style.transform.set(i, value.number());
                        }
                    }
                }
            }
            if let Some(slot) = self.slot_mut(id) {
                paint_view(&view, &style, slot, scroll);
                // The scene reads `slot.style`; keep the painted values so a
                // test tree shows where a pan or a running animation put it.
                slot.style = style;
            }
        }
    }

    pub(super) fn apply_item(&mut self, item: MountItem) {
        match item {
            MountItem::Create { id, kind } => self.create(id, kind),
            MountItem::Insert {
                parent,
                child,
                before,
            } => self.insert(parent, child, before),
            MountItem::Remove { id } => {
                self.detach_shadow_host(id);
                if self.remove_hosted(id) {
                    return;
                }
                self.detach_area(id);
                if let Some(view) = self.view(id) {
                    // Inside a modal that is dismissing: leave it for that
                    // animation. UIKit drops it with the controller.
                    if !crate::navigation::in_dismissing_modal(view) {
                        view.removeFromSuperview();
                    }
                }
            }
            MountItem::Delete { id } => {
                self.detach_area(id);
                self.forget_scroll(id);
                self.pending_pan.remove(&id);
                self.pending_pinch.remove(&id);
                self.pending_rotation.remove(&id);
                self.pending_bindings.remove(&id);
                if self.focused == Some(id) {
                    self.end_focus(id);
                }
                self.remove_hosted(id);
                if let Some(mut slot) = self.take(id) {
                    // A modal deleted while presented (its subtree released
                    // without a remove) goes away too, as on Android.
                    if let Some(host) = slot.shadow_host.take() {
                        host.removeFromSuperview();
                    }
                    if let Native::Modal(modal) = &mut slot.native {
                        modal.dismiss(|| {});
                    }
                    // An animated dismiss keeps the view. Removing it here
                    // would play the transition on an empty controller.
                    if !crate::navigation::in_dismissing_modal(slot.native.view()) {
                        slot.native.view().removeFromSuperview();
                    }
                }
            }
            MountItem::Frame { id, frame } => {
                self.frames_applied = true;
                if id == ROOT_ID {
                    return;
                }
                if let Some(Slot {
                    native: Native::Stack(stack),
                    ..
                }) = self.slot(id)
                {
                    stack.layout();
                }
                let header = if let Some(Slot {
                    native: Native::HeaderItem(view),
                    ..
                }) = self.slot(id)
                {
                    // The header bar positions it; only its size is ours.
                    let origin = view.frame().origin;
                    view.setFrame(CGRect::new(
                        origin,
                        CGSize::new(
                            frame.size.width.0 as CGFloat,
                            frame.size.height.0 as CGFloat,
                        ),
                    ));
                    true
                } else {
                    false
                };
                if header {
                    self.sync_shadow_host(id);
                    return;
                }
                // Corners, borders and the image frame depend on the size,
                // not where the view sits. A move leaves the paint alone.
                // The shadow host is not a subview, so it follows either way.
                let resized = self.view(id).is_some_and(|view| set_frame(view, frame));
                if resized {
                    self.repaint(id);
                }
                self.sync_shadow_host(id);
            }
            MountItem::ViewStyle { id, style } => {
                if let Some(slot) = self.slot_mut(id) {
                    slot.style = *style;
                }
                self.repaint(id);
            }
            MountItem::StateStyle {
                id,
                interaction,
                view,
                text,
            } => {
                let paint = (view.as_ref() != &ViewStyle::default()
                    || text != TextStyle::default())
                .then_some(Paint { view: *view, text });
                if let Some(slot) = self.slot_mut(id) {
                    slot.states[state_index(interaction)] = paint;
                }
                self.sync_hover(id);
                self.repaint(id);
            }
            MountItem::Text {
                id,
                text,
                layout,
                content,
            } => {
                let links = text
                    .links
                    .iter()
                    .any(|link| link.events.contains(EventKind::Press))
                    || !text.hrefs.is_empty();
                {
                    let Some(Native::Text(view)) = self.native(id) else {
                        return;
                    };
                    view.set_text(text, layout, content);
                }
                if let Some(slot) = self.slot_mut(id) {
                    slot.link_press = links;
                }
                self.sync_press(id);
            }
            MountItem::LongPressDelay { id, delay } => {
                if let Some(slot) = self.slot_mut(id) {
                    slot.delay_long = delay;
                }
            }
            MountItem::Events { id, events } => self.set_events(id, events),
            MountItem::ScrollConfig {
                id,
                horizontal,
                taps,
                dismiss,
            } => {
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                scroll.setAlwaysBounceHorizontal(horizontal);
                scroll.setAlwaysBounceVertical(!horizontal);
                scroll.setShowsHorizontalScrollIndicator(horizontal);
                scroll.setShowsVerticalScrollIndicator(!horizontal);
                scroll.setDirectionalLockEnabled(true);
                // A node that stops being vertical drops the width narrowed
                // while it was. Restoring that width would cut a horizontal
                // extent short, so the native size stays until the engine
                // sends the next content size.
                if !crate::scroll_extent::still_judged(horizontal) {
                    self.narrowed.retain(|&(n, ..)| n != id);
                }
                if let Some(slot) = self.slot_mut(id) {
                    slot.taps = taps;
                    slot.dismiss = dismiss;
                }
            }
            MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets,
                snap_alignment,
                disable_interval_momentum,
                deceleration,
            } => {
                let width = size.width.0;
                let height = size.height.0;

                // Insert content size
                self.relay
                    .ivars()
                    .content
                    .borrow_mut()
                    .insert(id, (width, height));

                // Check if we need to track content size
                let should_track = if let Some(Native::Scroll(scroll)) = self.native(id) {
                    crate::scroll_extent::still_judged(scroll.alwaysBounceHorizontal())
                } else {
                    false
                };
                if should_track {
                    self.content_sizes.push((id, height as CGFloat));
                }

                if let Some(slot) = self.slot_mut(id) {
                    slot.snap_paging = paging;
                    slot.snap_interval = snap_interval;
                    slot.snap_offsets = snap_offsets.clone();
                    slot.snap_alignment = snap_alignment;
                    slot.snap_one_step = disable_interval_momentum;
                    slot.snap_deceleration = deceleration;
                    slot.refresh_indicator = refresh_indicator.map(|points| points.0);
                }
                self.sync_refresh(id);

                // Now set scroll view properties
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                scroll.setContentSize(CGSize::new(width as CGFloat, height as CGFloat));

                // Calculate and set deceleration rate
                let behavior = nlayer_core::snap::SnapBehavior {
                    paging,
                    snap_interval: Points(snap_interval.unwrap_or(0.0)),
                    snap_offsets: snap_offsets
                        .as_ref()
                        .map(|offsets| offsets.iter().map(|&x| Points(x)).collect())
                        .unwrap_or_default(),
                    snap_alignment: match snap_alignment {
                        1 => nlayer_core::snap::SnapAlignment::Center,
                        2 => nlayer_core::snap::SnapAlignment::End,
                        _ => nlayer_core::snap::SnapAlignment::Start,
                    },
                    deceleration,
                    one_step: disable_interval_momentum,
                };
                let rate = nlayer_core::snap::rate(&behavior);
                scroll.setDecelerationRate(rate as CGFloat);
            }
            MountItem::ScrollBy { id, dx, dy } => {
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                let offset = scroll.contentOffset();
                scroll.setContentOffset(CGPoint::new(
                    offset.x + dx.0 as CGFloat,
                    offset.y + dy.0 as CGFloat,
                ));
            }
            MountItem::Command { id, command } => match command {
                nlayer_core::Command::ScrollTo { x, y, animated } => {
                    let Some(Native::Scroll(scroll)) = self.native(id) else {
                        return;
                    };
                    scroll.setContentOffset_animated(
                        clamped_scroll_offset(scroll, x.0, y.0),
                        animated,
                    );
                }
                nlayer_core::Command::Blur => self.end_editing(),
                nlayer_core::Command::SetSelection { start, end } => {
                    if let Some(Native::Input(field)) = self.native(id) {
                        crate::input::set_selection(field, start, end);
                    }
                }
                nlayer_core::Command::ScrollToIndex { .. }
                | nlayer_core::Command::ScrollToEnd { .. }
                | nlayer_core::Command::Back
                | nlayer_core::Command::GoBack => {}
                nlayer_core::Command::OpenUrl { url } => self.open_url(&url),
            },
            MountItem::Selectable { id, selectable } => self.set_selectable_area(id, selectable),
            MountItem::Image {
                id,
                source,
                resize_mode,
            } => self.set_image(id, source, resize_mode),
            MountItem::Input { id, config } => {
                self.set_input_multiline(id, config.multiline);
                match self.native(id) {
                    Some(Native::Input(field)) => field.configure(&config),
                    Some(Native::Area(area)) => area.configure(&config),
                    _ => {}
                }
                self.repaint(id);
            }
            MountItem::InputValue { id, text, count } => match self.native(id) {
                Some(Native::Input(field)) => field.set_value(&text, count),
                Some(Native::Area(area)) => area.set_value(&text, count),
                _ => {}
            },
            MountItem::InputContent { id, content } => {
                if let Some(slot) = self.slot_mut(id) {
                    slot.input_content = Some(content);
                }
                match self.native(id) {
                    Some(Native::Input(field)) => field.set_content(content),
                    Some(Native::Area(area)) => area.set_content(content),
                    _ => {}
                }
            }
            MountItem::ScreenConfig { id, config } => {
                let parent = self.hosts.get(&id).copied();
                let stored = if let Some(Slot {
                    native: Native::Screen(screen),
                    ..
                }) = self.slot_mut(id)
                {
                    screen.config = *config;
                    true
                } else {
                    false
                };
                if stored {
                    self.dirty_headers.push(id);
                    if let Some(parent) = parent {
                        self.dirty_stacks.push(parent);
                        // The page above shows this page's back button.
                        // Re-apply it so that item is refreshed from the
                        // values just stored.
                        let above = self.slot(parent).and_then(|slot| {
                            let Native::Stack(stack) = &slot.native else {
                                return None;
                            };
                            let pos = stack.screens.iter().position(|&screen| screen == id)?;
                            stack.screens.get(pos + 1).copied()
                        });
                        if let Some(above) = above {
                            self.dirty_headers.push(above);
                        }
                    }
                }
            }
            MountItem::ModalConfig { id, config } => {
                if let Some(Slot {
                    native: Native::Modal(modal),
                    ..
                }) = self.slot_mut(id)
                {
                    modal.configure(config);
                }
            }
            MountItem::Animation(op) => {
                self.animator.apply(op.clone());
                self.update_link();
            }
            MountItem::AnimationBindings { id, bindings } => {
                self.pending_bindings.insert(id, bindings.clone());
                if let Some(slot) = self.slot_mut(id) {
                    slot.bindings = bindings.clone();
                }
                self.apply_animations();
                self.update_link();
            }
            MountItem::TestId { id, test_id } => {
                if let Some(slot) = self.slot_mut(id) {
                    slot.test_id = test_id.clone();
                }
            }
            MountItem::Accessibility { id, accessibility } => {
                let mtm = self.mtm;
                let Some(slot) = self.slot_mut(id) else {
                    return;
                };
                slot.accessibility = (*accessibility).clone();
                let view = slot.native.view().retain();
                apply_accessibility(&view, &slot.accessibility, mtm);
            }
            MountItem::PanBinding { id, x, y } => {
                self.pending_pan.insert(id, (x, y));
                if let Some(slot) = self.slot_mut(id) {
                    slot.pan_nodes = (x, y);
                }
                self.sync_gestures(id);
            }
            MountItem::PinchBinding { id, scale } => {
                self.pending_pinch.insert(id, scale);
                if let Some(slot) = self.slot_mut(id) {
                    slot.pinch_node = scale;
                }
                self.sync_gestures(id);
            }
            MountItem::RotationBinding { id, angle } => {
                self.pending_rotation.insert(id, angle);
                if let Some(slot) = self.slot_mut(id) {
                    slot.rotation_node = angle;
                }
                self.sync_gestures(id);
            }
            MountItem::Refreshing { id, refreshing } => self.set_refreshing(id, refreshing),
            MountItem::ListContent { .. } => {
                // List content is handled by core layout
            }
            MountItem::TrackStates { id, states } => self.track_states(id, states),
            MountItem::PointerEvents { id, events } => self.set_pointer_events(id, events),
        }
    }
}
