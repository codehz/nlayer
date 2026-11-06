//! Style and prop application on the shadow tree.

use super::*;

impl<M: TextEngine> Core<M> {
    pub(super) fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), ApplyError> {
        let fit = style.object_fit();
        let key = self.key(id)?;
        let Style {
            mut layout,
            mut view,
            text,
            logical,
            z_index,
            vertical_align,
            direction,
            animated,
            pending,
            ..
        } = style;
        if key != self.root {
            self.nodes[key as usize].direction = direction;
        }
        self.nodes[key as usize].logical = (!logical.is_empty()).then(|| Box::new(logical));
        let rtl = self.resolved_rtl(key);
        let flipped = self.nodes[key as usize].rtl != rtl;
        style::resolve_normal_alignment(&mut layout);
        style::apply_direction(&mut layout, &mut view, &logical, rtl);
        let env_edges = pending.env;
        let applied = (!env_edges.is_empty()).then(|| self.env_insets(key, &env_edges));
        let node = &mut self.nodes[key as usize];
        let span_background_changed = node.inline
            && node.kind == Kind::Text
            && super::text::span_background(&node.view_style, node.id)
                != super::text::span_background(&view, node.id);
        let lines_changed = node.kind == Kind::Text
            && (node.number_of_lines != pending.number_of_lines
                || node.ellipsize != pending.text_overflow);
        let select_changed = node.user_select != pending.user_select;
        let align_changed = node.vertical_align != vertical_align;
        let text_changed = node.text_style != text || span_background_changed || lines_changed;
        node.rtl = rtl;
        let view_changed = node.view_style != view;
        let animation_changed = node.animated != animated;
        let z_changed = node.z_index != z_index;
        node.view_style = view;
        node.animated = animated;
        node.text_style = text;
        node.number_of_lines = pending.number_of_lines;
        node.ellipsize = pending.text_overflow;
        node.user_select = pending.user_select;
        let pointer = pending.pointer_events.filter(|p| *p != PointerEvents::Auto);
        let pointer_changed = node.explicit_pointer != pointer;
        node.explicit_pointer = pointer;
        node.vertical_align = vertical_align;
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
        let kind = node.kind;
        match applied {
            Some(applied) => node.env = Some(Box::new((env_edges, applied))),
            None => {
                node.env = None;
            }
        }
        let tracked = applied.is_some();
        if tracked {
            self.env_styled.insert(id);
        } else {
            self.env_styled.remove(&id);
        }
        if matches!(
            kind,
            Kind::Text | Kind::Scroll | Kind::List | Kind::Screen | Kind::Modal
        ) {
            self.nodes[key as usize].base_layout = Some(Box::new(layout.clone()));
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            // The scroll axis overrides the authored overflow. `env()` is
            // added there, on the physical edges, before a right-to-left
            // row swaps padding.
            self.sync_scroll_style(key);
        } else if let Some(l) = self.nodes[key as usize].layout {
            if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
                super::nav::add_insets(&mut layout, applied[0], applied[1]);
            }
            self.layout.set_style(l, layout);
            self.needs_layout = true;
        }
        if z_changed {
            self.restack(key);
        }
        if view_changed {
            self.mark(key, dirty::VIEW);
        }
        if animation_changed {
            self.mark(key, dirty::ANIMATION);
        }
        if pointer_changed {
            self.mark(key, dirty::POINTER);
        }
        if fit_changed {
            self.mark(key, dirty::IMAGE);
        }
        if text_changed || flipped || select_changed {
            match self.nodes[key as usize].kind {
                Kind::Text => self.mark_text_block(key),
                Kind::Input => {
                    self.measure_input(key);
                    self.mark(key, dirty::INPUT);
                }
                _ if text_changed || select_changed => self.mark_inheriting_text(key),
                _ => {}
            }
        }
        if align_changed && self.nodes[key as usize].inline {
            // Its line reserves its new place.
            self.mark_text_block(key);
        }
        if flipped {
            self.update_children_direction(key);
        }
        Ok(())
    }

    pub(super) fn set_prop(&mut self, id: NodeId, prop: Prop) -> Result<(), ApplyError> {
        let key = self.key(id)?;
        match prop {
            Prop::Source(source) => {
                let Some(image) = self.nodes[key as usize].image.as_mut() else {
                    return Ok(());
                };
                let size = source.as_ref().and_then(|s| s.size);
                *image = source;
                self.mark(key, dirty::IMAGE);
                let Some(layout) = self.nodes[key as usize].layout else {
                    return Ok(());
                };
                if let Some(LeafContext::Image(natural)) = self.layout.get_node_context_mut(layout)
                    && *natural != size
                {
                    *natural = size;
                    self.layout.mark_dirty(layout);
                    self.needs_layout = true;
                }
            }
            Prop::Horizontal(horizontal) => {
                if self.nodes[key as usize].kind == Kind::List {
                    if horizontal {
                        log::warn!("a v2 list scrolls vertically; `horizontal` is ignored");
                    }
                    return Ok(());
                }
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.horizontal != horizontal;
                        scroll.horizontal = horizontal;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                    self.sync_scroll_style(key);
                }
            }
            Prop::KeyboardTaps(taps) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.taps != taps;
                        scroll.taps = taps;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::KeyboardDismiss(mode) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.dismiss != mode;
                        scroll.dismiss = mode;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::Refreshing(refreshing) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let was_refreshing = scroll.refreshing;
                        scroll.refreshing = refreshing;
                        // Reset reported flag on transitions
                        if was_refreshing != refreshing {
                            scroll.refresh_reported = false;
                        }
                        was_refreshing != refreshing
                    });
                if changed {
                    self.mark(key, dirty::REFRESHING);
                }
            }
            Prop::RefreshIndicator(height) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.refresh_indicator != height;
                        scroll.refresh_indicator = height;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::PullValue(node) => {
                if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
                    scroll.pull_value_node = node;
                }
            }
            Prop::PagingEnabled(paging) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.paging != paging;
                        scroll.paging = paging;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToInterval(interval) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_interval != interval;
                        scroll.snap_interval = interval;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToOffsets(offsets) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_offsets != offsets;
                        scroll.snap_offsets = offsets;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToAlignment(alignment) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_alignment != alignment;
                        scroll.snap_alignment = alignment;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::DisableIntervalMomentum(disable) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.disable_interval_momentum != disable;
                        scroll.disable_interval_momentum = disable;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::DecelerationRate(rate) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let value = rate.value();
                        let changed = (scroll.deceleration - value).abs() > 0.001;
                        scroll.deceleration = value;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::Count(count) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_count(count);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::EstimatedItemSize(size) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_estimated(size.0);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Columns(columns) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_columns(columns);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Splice {
                at,
                removed,
                inserted,
            } => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.splice(at, removed, inserted);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Slot(slot) => {
                self.nodes[key as usize].slot = Some(slot);
                let parent = self.nodes[key as usize].parent;
                if parent != NO_KEY
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.add_slot_cell(slot, key);
                    self.mark(parent, dirty::LIST);
                }
            }
            Prop::CellIndex(index) => {
                let parent = self.nodes[key as usize].parent;
                if parent != NO_KEY
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.rendered(key, index);
                    self.mark(parent, dirty::LIST);
                }
            }
            Prop::LoadMoreThreshold(threshold) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_end_threshold(threshold);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::StickyHeaderIndices(indices) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_sticky(indices.map(|b| b.into_vec()).unwrap_or_default());
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Inverted(inverted) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_inverted(inverted);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::MaintainVisibleContentPosition(config) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.maintain_position = config;
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Viewport(viewport) => {
                self.set_list_viewport(key, matches!(viewport, crate::ops::ListViewport::Parent));
            }
            Prop::ViewabilityConfig(config) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.viewability = config;
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Value(value) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                if input.value != value {
                    input.value = value;
                    self.mark(key, dirty::INPUT_VALUE);
                    self.measure_input(key);
                }
            }
            Prop::Placeholder(text) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.placeholder = text;
                self.mark(key, dirty::INPUT);
                self.measure_input(key);
            }
            Prop::Secure(secure) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.secure = secure;
                self.mark(key, dirty::INPUT);
            }
            Prop::Keyboard(keyboard) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.keyboard = keyboard;
                self.mark(key, dirty::INPUT);
            }
            Prop::Multiline(multiline) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                if input.config.multiline != multiline {
                    input.config.multiline = multiline;
                    self.mark(key, dirty::INPUT);
                    self.measure_input(key);
                }
            }
            Prop::Selection { start, end } => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.selection = Some((start, end));
                self.mark(key, dirty::SELECTION);
            }
            Prop::LongPressDelay(delay) => {
                if self.nodes[key as usize].long_press_delay != delay {
                    self.nodes[key as usize].long_press_delay = delay;
                    self.mark(key, dirty::EVENTS);
                }
            }
            Prop::Disabled(disabled) => {
                if let Some(input) = self.nodes[key as usize].input.as_mut() {
                    input.config.disabled = disabled;
                    self.mark(key, dirty::INPUT);
                }
                if self.nodes[key as usize].accessibility.state.disabled != disabled {
                    self.nodes[key as usize].accessibility.state.disabled = disabled;
                    self.mark(key, dirty::ACCESSIBILITY);
                }
                self.set_disabled(key, disabled);
            }
            Prop::Accessibility(prop) => {
                let node = &mut self.nodes[key as usize];
                let accessibility = &mut node.accessibility;
                let changed = match prop {
                    AccessibilityProp::Label(label) => {
                        let changed = accessibility.label != label;
                        accessibility.label = label;
                        changed
                    }
                    AccessibilityProp::Hint(hint) => {
                        let changed = accessibility.hint != hint;
                        accessibility.hint = hint;
                        changed
                    }
                    AccessibilityProp::Role(role) => {
                        let changed = accessibility.role != role;
                        accessibility.role = role;
                        changed
                    }
                    AccessibilityProp::State(state) => {
                        let changed = accessibility.state != state;
                        accessibility.state = state;
                        changed
                    }
                    AccessibilityProp::Value(value) => {
                        let changed = accessibility.value != value;
                        accessibility.value = value;
                        changed
                    }
                    AccessibilityProp::Hidden(hidden) => {
                        let changed = accessibility.hidden != hidden;
                        accessibility.hidden = hidden;
                        changed
                    }
                };
                if changed {
                    self.mark(key, dirty::ACCESSIBILITY);
                }
            }
            Prop::TestId(value) => {
                let node = &mut self.nodes[key as usize];
                if node.test_id == value {
                    return Ok(());
                }
                let inline = node.kind == Kind::Text && node.inline;
                node.test_id = value;
                if inline {
                    self.mark_text_block(key);
                } else {
                    self.mark(key, dirty::TEST_ID);
                }
            }
            Prop::StateStyle { interaction, paint } => {
                let slot = interaction.index();
                let next = paint.map(|paint| StatePaint {
                    view: paint.0,
                    text: paint.1,
                });
                if self.nodes[key as usize].state[slot] != next {
                    self.nodes[key as usize].state[slot] = next;
                    self.nodes[key as usize].state_dirty |= 1 << slot;
                    self.mark(key, dirty::STATE);
                }
            }
            Prop::Screen(screen_prop) => {
                use crate::ops::ScreenProp;
                let mut cover_changed = false;
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    cover_changed = match &screen_prop {
                        ScreenProp::HeaderVisible(visible) => {
                            root.screen.header_visible != *visible
                        }
                        ScreenProp::StandardAppearance(appearance) => {
                            let was = root.screen.header_transparent;
                            let next = !appearance
                                .background_color
                                .is_some_and(|color| !color.is_transparent());
                            was != next
                        }
                        ScreenProp::Subtitle(_) | ScreenProp::LargeTitle(_) => true,
                        _ => false,
                    };
                    let config = &mut root.screen;
                    match screen_prop {
                        ScreenProp::Title(title) => config.title = title,
                        ScreenProp::Subtitle(subtitle) => config.subtitle = subtitle,
                        ScreenProp::HeaderVisible(visible) => config.header_visible = visible,
                        ScreenProp::LargeTitle(mode) => config.large_title = mode,
                        ScreenProp::BackButtonTitle(title) => config.back_button_title = title,
                        ScreenProp::BackButtonDisplayMode(mode) => {
                            config.back_button_display_mode = mode
                        }
                        ScreenProp::HidesBackButton(hides) => config.hides_back_button = hides,
                        ScreenProp::LeftItemsSupplementBackButton(supplement) => {
                            config.left_items_supplement_back_button = supplement
                        }
                        ScreenProp::PopOnSwipe(enabled) => config.gesture_enabled = enabled,
                        ScreenProp::PopOnBack(pop) => config.prevent_back = !pop,
                        ScreenProp::Animation(animation) => config.animation = animation,
                        ScreenProp::StandardAppearance(appearance) => config.standard = appearance,
                        ScreenProp::ScrollEdgeAppearance(appearance) => {
                            config.scroll_edge = appearance
                        }
                    }
                    config.sync_cover();
                    self.nodes[key as usize]
                        .root
                        .as_mut()
                        .expect("root")
                        .laid_out = None;
                    self.nodes[key as usize].dirty |= dirty::SCREEN;
                    self.needs_layout = true;
                }
                if cover_changed {
                    self.sync_env();
                }
            }
            Prop::Modal(modal_prop) => {
                use crate::ops::ModalProp;
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    let config = &mut root.modal;
                    match modal_prop {
                        ModalProp::Animation(animation) => config.animation = animation,
                        ModalProp::Presentation(presentation) => config.presentation = presentation,
                        ModalProp::CloseOnEscape(close) => config.close_on_escape = close,
                        ModalProp::CloseOnBackdropPress(close) => {
                            config.close_on_backdrop_press = close
                        }
                        ModalProp::CloseOnSwipe(close) => config.close_on_swipe = close,
                    }
                    self.nodes[key as usize].dirty |= dirty::MODAL;
                }
            }
            Prop::Pan { x, y } => {
                self.nodes[key as usize].gestures.pan_x = x;
                self.nodes[key as usize].gestures.pan_y = y;
                self.mark(key, dirty::PAN);
            }
            Prop::Pinch { scale } => {
                self.nodes[key as usize].gestures.pinch = scale;
                self.mark(key, dirty::PAN);
            }
            Prop::Rotation { angle } => {
                self.nodes[key as usize].gestures.rotation = angle;
                self.mark(key, dirty::PAN);
            }
            Prop::Selectable(selectable) => {
                if self.nodes[key as usize].selectable != selectable {
                    self.nodes[key as usize].selectable = selectable;
                    self.mark(key, dirty::SELECTABLE);
                }
            }
            Prop::Href(href) => {
                let href = href.map(Arc::from);
                if self.nodes[key as usize].href != href {
                    self.nodes[key as usize].href = href;
                    self.mark_text_block(key);
                }
            }
            Prop::Class(name) => self.set_class_name(key, name),
            Prop::ClassList(list) => self.set_class_list_names(key, list),
            Prop::HeaderSlot(slot) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.slot = slot;
                    let parent = self.nodes[key as usize].parent;
                    if parent != NO_KEY {
                        self.needs_layout = true;
                    }
                }
            }
            Prop::HeaderGroup(group) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.group = group;
                }
                self.mark(key, dirty::SCREEN);
            }
            Prop::HeaderIndex(index) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.index = index;
                }
            }
            Prop::BarButton(button) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.button = button;
                    if button {
                        root.bar_enabled = true;
                    }
                }
            }
            Prop::BarTitle(title) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.bar_title = title;
                }
            }
            Prop::BarImage(image) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.bar_image = image;
                }
            }
            Prop::BarEnabled(enabled) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.bar_enabled = enabled;
                }
            }
            Prop::BarTint(tint) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.bar_tint = tint;
                }
            }
        }
        Ok(())
    }

    /// The pixel ratio went from `old` to `new`: the images whose variant
    /// changes are sent again.
    pub(super) fn repick_images(&mut self, old: f32, new: f32) {
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
    pub(super) fn resolved_rtl(&self, key: Key) -> bool {
        let node = &self.nodes[key as usize];
        if let Some(rtl) = node.direction {
            return rtl;
        }
        match node.parent {
            NO_KEY => key != self.root && self.nodes[self.root as usize].rtl,
            parent => self.nodes[parent as usize].rtl,
        }
    }

    /// Re-resolves the direction of `key` (moved, or its parent's changed)
    /// and of the nodes inheriting it.
    pub(super) fn update_direction(&mut self, key: Key) {
        let rtl = self.resolved_rtl(key);
        if self.nodes[key as usize].rtl == rtl {
            return;
        }
        self.flip_direction(key, rtl);
        self.update_children_direction(key);
    }

    fn update_children_direction(&mut self, key: Key) {
        let children = self.nodes[key as usize].children.clone();
        for child in children {
            self.update_direction(child);
        }
    }

    /// Lays `key` out in the other direction.
    fn flip_direction(&mut self, key: Key, rtl: bool) {
        let direction = if rtl {
            taffy::Direction::Rtl
        } else {
            taffy::Direction::Ltr
        };
        self.nodes[key as usize].rtl = rtl;
        let kind = self.nodes[key as usize].kind;
        let layout_id = self.nodes[key as usize].layout;
        if matches!(kind, Kind::Scroll | Kind::List) {
            if let Some(base) = self.nodes[key as usize].base_layout.as_mut() {
                base.direction = direction;
            }
            self.sync_scroll_style(key);
        } else if let Some(l) = layout_id {
            let mut layout = self.layout.style(l).clone();
            layout.direction = direction;
            self.layout.set_style(l, layout);
            self.needs_layout = true;
        }
        self.mark(key, dirty::VIEW);
        if kind == Kind::Text {
            self.mark_text_block(key);
        }
        if kind == Kind::Input {
            self.measure_input(key);
            self.mark(key, dirty::INPUT);
        }
    }
}
