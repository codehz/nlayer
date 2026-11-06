//! Press, hover, pan, pinch and rotation recognizers.

use dispatch2::{DispatchQueue, DispatchTime};
use nlayer_core::tailwind::state::{FOCUSED, HOVERED, PRESSED};
use nlayer_core::{EventKind, Interaction, NodeId};
use nlayer_engine::EventPayload;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadOnly, Message, sel};
use objc2_core_foundation::CGPoint;
use objc2_ui_kit::{
    UIGestureRecognizer, UIGestureRecognizerState, UIHoverGestureRecognizer,
    UILongPressGestureRecognizer, UIPanGestureRecognizer, UIPinchGestureRecognizer,
    UIRefreshControl, UIRotationGestureRecognizer, UIScrollView, UIView,
};

use super::*;

impl Mounter {
    pub(super) fn set_events(&mut self, id: NodeId, events: nlayer_core::EventMask) {
        if let Some(slot) = self.slot_mut(id) {
            slot.events = events;
            slot.own_press = events.contains(EventKind::Press);
            slot.own_long = events.contains(EventKind::LongPress);
        }
        self.sync_refresh(id);
        self.sync_press(id);
        self.sync_hover(id);
        self.sync_gestures(id);
    }

    pub(super) fn sync_press(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let want = slot.own_press || slot.link_press || slot.own_long;
        let view = slot.native.view().retain();
        let mut released = false;
        match (&slot.press, want) {
            (None, true) => {
                // SAFETY: the relay implements `pressed:`.
                let gesture = unsafe {
                    UILongPressGestureRecognizer::initWithTarget_action(
                        UILongPressGestureRecognizer::alloc(mtm),
                        Some(&relay),
                        Some(sel!(pressed:)),
                    )
                };
                gesture.setMinimumPressDuration(0.0);
                gesture.setCancelsTouchesInView(false);
                gesture.setDelegate(Some(ProtocolObject::from_ref(&*relay)));
                view.addGestureRecognizer(&gesture);
                slot.press = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.press = None;
                slot.pressed = false;
                released = true;
            }
            _ => {}
        }
        if released {
            self.report_state(id, PRESSED, false);
        }
    }

    pub(super) fn sync_hover(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let view = slot.native.view().retain();
        let want_hover = slot.states[state_index(Interaction::Hover)].is_some();
        let mut left = false;
        match (&slot.hover, want_hover) {
            (None, true) => {
                // SAFETY: the relay implements `hovered:`.
                let gesture = unsafe {
                    UIHoverGestureRecognizer::initWithTarget_action(
                        UIHoverGestureRecognizer::alloc(mtm),
                        Some(&relay),
                        Some(sel!(hovered:)),
                    )
                };
                view.addGestureRecognizer(&gesture);
                slot.hover = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.hover = None;
                slot.hovered = false;
                left = true;
            }
            _ => {}
        }
        if left {
            self.report_state(id, HOVERED, false);
        }
    }

    pub(super) fn sync_gestures(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let events = slot.events;
        let (pan_x, pan_y) = slot.pan_nodes;
        let pinch_node = slot.pinch_node;
        let rotation_node = slot.rotation_node;
        let view = slot.native.view().retain();
        let delegate = ProtocolObject::from_ref(&*relay);

        let want_pan = pan_x.is_some()
            || pan_y.is_some()
            || events.contains(EventKind::PanStart)
            || events.contains(EventKind::PanEnd);
        match (&slot.pan_recognizer, want_pan) {
            (None, true) => {
                // SAFETY: the relay implements `handlePan:`.
                let gesture = unsafe {
                    UIPanGestureRecognizer::initWithTarget_action(
                        UIPanGestureRecognizer::alloc(mtm),
                        Some(&relay),
                        Some(sel!(handlePan:)),
                    )
                };
                gesture.setDelegate(Some(delegate));
                view.addGestureRecognizer(&gesture);
                slot.pan_recognizer = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.pan_recognizer = None;
            }
            _ => {}
        }

        let want_pinch = pinch_node.is_some()
            || events.contains(EventKind::PinchStart)
            || events.contains(EventKind::PinchEnd);
        match (&slot.pinch_recognizer, want_pinch) {
            (None, true) => {
                // SAFETY: the relay implements `handlePinch:`.
                let gesture = unsafe {
                    UIPinchGestureRecognizer::initWithTarget_action(
                        UIPinchGestureRecognizer::alloc(mtm),
                        Some(&relay),
                        Some(sel!(handlePinch:)),
                    )
                };
                gesture.setDelegate(Some(delegate));
                view.addGestureRecognizer(&gesture);
                slot.pinch_recognizer = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.pinch_recognizer = None;
            }
            _ => {}
        }

        let want_rotation = rotation_node.is_some()
            || events.contains(EventKind::RotationStart)
            || events.contains(EventKind::RotationEnd);
        match (&slot.rotation_recognizer, want_rotation) {
            (None, true) => {
                // SAFETY: the relay implements `handleRotation:`.
                let gesture = unsafe {
                    UIRotationGestureRecognizer::initWithTarget_action(
                        UIRotationGestureRecognizer::alloc(mtm),
                        Some(&relay),
                        Some(sel!(handleRotation:)),
                    )
                };
                gesture.setDelegate(Some(delegate));
                view.addGestureRecognizer(&gesture);
                slot.rotation_recognizer = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.rotation_recognizer = None;
            }
            _ => {}
        }
    }

    /// `none` returns nil from `BoxView`'s hit test and stays interaction
    /// enabled, so a scroll view's hit test falls through to itself (nsh).
    /// Turning interaction off drops the touch instead. Inputs are the
    /// exception: `none` and `box-none` must not become first responder.
    pub(super) fn set_pointer_events(&mut self, id: NodeId, events: nlayer_core::PointerEvents) {
        let input = self
            .slot(id)
            .is_some_and(|slot| matches!(slot.native, Native::Input(_) | Native::Area(_)));
        let Some(view) = self.view(id) else {
            return;
        };
        if let Some(box_view) = view.downcast_ref::<BoxView>() {
            box_view.set_pointer(events);
        } else if let Some(text) = view.downcast_ref::<crate::text_view::TextView>() {
            text.set_pointer(events);
        }
        if input {
            view.setUserInteractionEnabled(!matches!(
                events,
                nlayer_core::PointerEvents::None | nlayer_core::PointerEvents::BoxNone
            ));
        }
    }

    /// Stores which states of `id` the engine reads. `0` stops press and
    /// hover; a focus change is still reported.
    pub(super) fn track_states(&mut self, id: NodeId, states: u16) {
        let reported = {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            if slot.tracked == states {
                return;
            }
            slot.tracked = states;
            if slot.accessibility.blocks_press() {
                slot.bits &= !PRESSED;
            }
            let reported = slot.bits & (states | FOCUSED);
            // A press or hover already held when tracking starts. Focus was
            // reported when it changed; it rides along so this does not clear it.
            if reported & (PRESSED | HOVERED) == 0 {
                return;
            }
            reported
        };
        if let Some(control) = &self.control {
            control.set_states(id, reported);
        }
    }

    /// `bit` of `id` turned on or off. Press and hover are sent only when
    /// `tracked` asks for them; focus always is. The value keeps every other
    /// bit the slot still holds, so a press does not clear a hover, then
    /// drops anything the engine did not ask for (`tracked | FOCUSED`).
    pub(super) fn report_state(&mut self, id: NodeId, bit: u16, on: bool) {
        let reported = {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            let mut next = if on {
                slot.bits | bit
            } else {
                slot.bits & !bit
            };
            // Disabled is already on the slot: do not report a press.
            if slot.accessibility.blocks_press() {
                next &= !PRESSED;
            }
            if next == slot.bits {
                return;
            }
            slot.bits = next;
            let mask = slot.tracked | FOCUSED;
            if mask & bit == 0 {
                return;
            }
            next & mask
        };
        if let Some(control) = &self.control {
            control.set_states(id, reported);
        }
    }

    pub(super) fn pressed(&mut self, id: NodeId, state: UIGestureRecognizerState, point: CGPoint) {
        if state == UIGestureRecognizerState::Began && self.press_blocked(id, point) {
            return;
        }
        let began = state == UIGestureRecognizerState::Began;
        let on = matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        );
        let ended = state == UIGestureRecognizerState::Ended;
        let cancelled = state == UIGestureRecognizerState::Cancelled
            || state == UIGestureRecognizerState::Failed;
        let mut long = None;
        let mut fired = false;
        let takes_selection = began
            && self
                .view(id)
                .is_some_and(|view| crate::text_view::selection_takes_long_press(view, point));
        if self.slot(id).is_some_and(|slot| slot.press_cancelled) && !began {
            if let Some(slot) = self.slot_mut(id) {
                slot.pressed = false;
                if ended || cancelled {
                    slot.press_cancelled = false;
                    slot.long_generation = slot.long_generation.wrapping_add(1);
                }
            }
            self.repaint(id);
            self.report_state(id, PRESSED, false);
            return;
        }
        if let Some(slot) = self.slot_mut(id) {
            if began {
                slot.press_cancelled = false;
                slot.long_generation = slot.long_generation.wrapping_add(1);
                slot.long_fired = false;
                if slot.own_long && !takes_selection {
                    long = Some((slot.long_generation, slot.delay_long));
                }
            }
            fired = slot.long_fired;
            slot.pressed = on;
            if ended || cancelled {
                slot.long_generation = slot.long_generation.wrapping_add(1);
            }
        }
        self.report_state(id, PRESSED, on);
        if let Some((generation, delay)) = long {
            let delay = delay.unwrap_or(500.0).max(0.0);
            let after = DispatchTime::try_from(std::time::Duration::from_secs_f64(
                f64::from(delay) / 1000.0,
            ))
            .unwrap_or(DispatchTime::NOW);
            let _ = DispatchQueue::main().after(after, move || {
                let Some(mounter) = current() else {
                    return;
                };
                mounter.borrow_mut().long_press(id, generation);
            });
        }
        self.repaint(id);
        if !ended {
            return;
        }
        if fired {
            return;
        }
        let target = self.link_target(id, point);
        let own = self.slot(id).is_some_and(|slot| slot.own_press);
        let Some(control) = self.control.clone() else {
            return;
        };
        if let Some(link) = target {
            control.dispatch_event(link, EventKind::Press, nlayer_engine::EventPayload::None);
        } else if own {
            control.dispatch_event(id, EventKind::Press, nlayer_engine::EventPayload::None);
        }
    }

    /// Selection started on `id`: the pending long press never fires, and
    /// the release is not `onPress` or an `href`.
    pub(crate) fn cancel_press(&mut self, id: NodeId) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        slot.long_generation = slot.long_generation.wrapping_add(1);
        slot.pressed = false;
        slot.long_fired = false;
        slot.press_cancelled = true;
        self.repaint(id);
        self.report_state(id, PRESSED, false);
    }

    /// Cancels the press of `view` and of every ancestor that has one.
    pub(crate) fn cancel_press_chain(&mut self, view: &UIView) {
        let mut current = Some(view.retain());
        while let Some(v) = current {
            let id = v.tag() as NodeId;
            if self
                .slot(id)
                .is_some_and(|slot| std::ptr::eq(slot.native.view(), &*v))
            {
                self.cancel_press(id);
            }
            current = v.superview();
        }
    }

    /// Attaches or removes the selection of a selectable view.
    pub(super) fn set_selectable_area(&mut self, id: NodeId, selectable: bool) {
        let existing = self.areas.iter().position(|(node, _)| *node == id);
        if !selectable {
            if let Some(index) = existing {
                let (_, area) = self.areas.remove(index);
                area.detach();
            }
            return;
        }
        if existing.is_some() {
            return;
        }
        let Some(view) = self.view(id).map(|view| view.retain()) else {
            return;
        };
        let area = crate::selectable_view::AreaResponder::attach(&view, self.mtm);
        self.areas.push((id, area));
    }

    /// Drops a selectable view's selection when the view goes.
    pub(super) fn detach_area(&mut self, id: NodeId) {
        if let Some(index) = self.areas.iter().position(|(node, _)| *node == id) {
            let (_, area) = self.areas.remove(index);
            area.detach();
        }
    }

    /// The URLs short presses on `Text` `href`s opened, in order.
    pub(crate) fn opened_urls(&self) -> &[String] {
        &self.opened_urls
    }

    /// Opens `url` with the system. One way: no result comes back. A test
    /// run (a pin) records it instead of leaving the app.
    pub(super) fn open_url(&mut self, url: &str) {
        self.opened_urls.push(url.to_owned());
        if self.pin.is_some() {
            return;
        }
        let Some(url) =
            objc2_foundation::NSURL::URLWithString(&objc2_foundation::NSString::from_str(url))
        else {
            return;
        };
        let options = objc2_foundation::NSDictionary::<
            objc2_ui_kit::UIApplicationOpenExternalURLOptionsKey,
            objc2::runtime::AnyObject,
        >::new();
        // SAFETY: an empty options dictionary and no completion handler.
        unsafe {
            objc2_ui_kit::UIApplication::sharedApplication(self.mtm)
                .openURL_options_completionHandler(&url, &options, None);
        }
    }

    pub(super) fn long_press(&mut self, id: NodeId, generation: u64) {
        let fire = self.slot_mut(id).is_some_and(|slot| {
            if slot.pressed && slot.long_generation == generation && !slot.long_fired {
                slot.long_fired = true;
                true
            } else {
                false
            }
        });
        if !fire {
            return;
        }
        if let Some(control) = &self.control {
            control.dispatch_event(id, EventKind::LongPress, nlayer_engine::EventPayload::None);
        }
    }

    /// A disabled view is under `point`, from the recognizer's view down:
    /// the press stops there and does not reach an ancestor.
    pub(super) fn press_blocked(&self, id: NodeId, point: CGPoint) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        if slot.accessibility.blocks_press() {
            return true;
        }
        let view = slot.native.view().retain();
        let Some(hit) = view.hitTest_withEvent(point, None) else {
            return false;
        };
        let mut current = Some(hit);
        while let Some(v) = current {
            let vid = v.tag() as NodeId;
            if let Some(slot) = self
                .slot(vid)
                .filter(|slot| std::ptr::eq(slot.native.view(), &*v))
                && slot.accessibility.blocks_press()
            {
                return true;
            }
            if std::ptr::eq(&*v, &*view) {
                break;
            }
            current = v.superview();
        }
        false
    }

    pub(super) fn link_target(&self, id: NodeId, point: CGPoint) -> Option<NodeId> {
        let Native::Text(view) = self.native(id)? else {
            return None;
        };
        view.link_at(point)
    }

    pub(super) fn hovered(&mut self, id: NodeId, state: UIGestureRecognizerState) {
        let on = matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        );
        if let Some(slot) = self.slot_mut(id) {
            slot.hovered = on;
        }
        self.report_state(id, HOVERED, on);
        self.repaint(id);
    }

    /// The test driver hovers `id` the way the hover gesture does.
    pub(crate) fn test_hover(&mut self, id: NodeId) {
        self.hovered(id, UIGestureRecognizerState::Began);
    }

    pub(super) fn repaint(&mut self, id: NodeId) {
        // Gestures and image arrivals are outside the batch transaction.
        quiet(|| {
            let (view, paint, text_color, scroll) = {
                let Some(slot) = self.slot(id) else { return };
                (
                    slot.native.view().retain(),
                    painted(slot),
                    text_override(slot),
                    matches!(slot.native, Native::Scroll(_)),
                )
            };
            if let Some(slot) = self.slot_mut(id) {
                paint_view(&view, &paint, slot, scroll);
                place_image(&view, &paint, slot);
            }
            if let Some(Native::Text(text)) = self.native(id) {
                text.set_color(text_color);
            }
            match self.native(id) {
                Some(Native::Input(field)) => field.set_override_color(text_color),
                Some(Native::Area(area)) => area.set_override_color(text_color),
                _ => {}
            }
        });
    }

    pub(super) fn pan_should_begin(&self, id: NodeId, velocity: CGPoint) -> bool {
        let Some(slot) = self.slot(id) else {
            return true;
        };
        match slot.pan_nodes {
            (Some(_), None) => velocity.x.abs() > velocity.y.abs(),
            (None, Some(_)) => velocity.y.abs() > velocity.x.abs(),
            _ => true,
        }
    }

    pub(super) fn pan_touch(&mut self, id: NodeId, touched: &UIView) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        slot.pan_scroll = None;
        let (pan_x, pan_y) = slot.pan_nodes;
        if !(pan_x.is_some() ^ pan_y.is_some()) {
            return;
        }
        let panel = slot.native.view();
        let mut current = Some(touched.retain());
        while let Some(view) = current {
            if &*view == panel {
                break;
            }
            if let Some(scroll) = view.downcast_ref::<UIScrollView>()
                && scroll.isScrollEnabled()
                && (scroll.contentSize().height > scroll.bounds().size.height
                    || scroll.alwaysBounceVertical())
            {
                slot.pan_scroll = Some(PanScroll {
                    view: scroll.retain(),
                    offset: (scroll.contentOffset().y + scroll.adjustedContentInset().top).max(0.0)
                        as f32,
                });
                break;
            }
            current = view.superview();
        }
    }

    pub(super) fn pan_shares_scroll(
        &self,
        pan: &UIGestureRecognizer,
        other: &UIGestureRecognizer,
    ) -> bool {
        let Some(view) = pan.view() else { return false };
        let Some(slot) = self.slot(view.tag() as NodeId) else {
            return false;
        };
        slot.pan_nodes.0.is_some() ^ slot.pan_nodes.1.is_some()
            && slot
                .pan_recognizer
                .as_deref()
                .is_some_and(|own| std::ptr::eq(own as &UIGestureRecognizer, pan))
            && slot.pan_scroll.as_ref().is_some_and(|scroll| {
                std::ptr::eq(
                    &*scroll.view.panGestureRecognizer() as &UIGestureRecognizer,
                    other,
                )
            })
    }

    pub(super) fn pan_scroll_offset(&self, scroll: &UIScrollView, offset: CGPoint) -> CGPoint {
        let mut current = scroll.superview();
        while let Some(view) = current {
            if let Some(slot) = self.slot(view.tag() as NodeId)
                && let Some(pan_scroll) = &slot.pan_scroll
                && &*pan_scroll.view == scroll
                && let Some(pan) = &slot.pan_recognizer
                && (matches!(
                    pan.state(),
                    UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
                ) || pan.state() == UIGestureRecognizerState::Ended && scroll.isDragging())
            {
                let t = pan.translationInView(view.superview().as_deref());
                if slot.gesture_start[1] + t.y as f32 > pan_scroll.offset {
                    return CGPoint::new(offset.x, -scroll.adjustedContentInset().top);
                }
            }
            current = view.superview();
        }
        offset
    }

    pub(super) fn handle_pan(
        &mut self,
        id: NodeId,
        state: UIGestureRecognizerState,
        translation: CGPoint,
        velocity: CGPoint,
    ) {
        let Some(slot) = self.slot(id) else { return };
        let (pan_x, pan_y) = slot.pan_nodes;
        let events = slot.events;

        if state == UIGestureRecognizerState::Began {
            // Read current values and stop animations (set does both)
            let x = pan_x.map(|n| self.animator.value(n)).unwrap_or(0.0);
            let y = pan_y.map(|n| self.animator.value(n)).unwrap_or(0.0);

            if let Some(slot) = self.slot_mut(id) {
                slot.gesture_start[0] = x;
                slot.gesture_start[1] = y;
            }

            if events.contains(EventKind::PanStart) {
                let payload = EventPayload::Pan {
                    translation: (translation.x as f32, translation.y as f32),
                    velocity: (velocity.x as f32, velocity.y as f32),
                    values: (x, y),
                };
                if let Some(control) = &self.control {
                    control.dispatch_event(id, EventKind::PanStart, payload);
                }
            }
        }

        if matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        ) {
            let start = self.slot(id).map_or([0.0; 4], |s| s.gesture_start);
            if let Some(n) = pan_x {
                self.animator.set(n, start[0] + translation.x as f32);
            }
            if let Some(n) = pan_y {
                let y = self
                    .slot(id)
                    .and_then(|s| s.pan_scroll.as_ref())
                    .map_or(start[1] + translation.y as f32, |scroll| {
                        (start[1] + translation.y as f32 - scroll.offset).max(0.0)
                    });
                self.animator.set(n, y);
                // Clamp scroll offset to keep it at top while panel consumes drag
                if let Some(scroll) = self.slot(id).and_then(|s| s.pan_scroll.as_ref()) {
                    let offset = scroll.view.contentOffset();
                    let clamped = self.pan_scroll_offset(&scroll.view, offset);
                    if clamped != offset {
                        scroll.view.setContentOffset(clamped);
                    }
                }
            }
            self.apply_animations();
        }

        if matches!(
            state,
            UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled
        ) && events.contains(EventKind::PanEnd)
        {
            let x = pan_x.map(|n| self.animator.value(n)).unwrap_or(0.0);
            let y = pan_y.map(|n| self.animator.value(n)).unwrap_or(0.0);

            let payload = EventPayload::Pan {
                translation: (translation.x as f32, translation.y as f32),
                velocity: (velocity.x as f32, velocity.y as f32),
                values: (x, y),
            };
            if let Some(control) = &self.control {
                control.dispatch_event(id, EventKind::PanEnd, payload);
            }
        }
    }

    pub(super) fn handle_pinch(
        &mut self,
        id: NodeId,
        state: UIGestureRecognizerState,
        scale: f32,
        velocity: f32,
    ) {
        let Some(slot) = self.slot(id) else { return };
        let node = slot.pinch_node;
        let events = slot.events;

        if state == UIGestureRecognizerState::Began {
            let start = node.map(|n| self.animator.value(n)).unwrap_or(1.0);
            if let Some(slot) = self.slot_mut(id) {
                slot.gesture_start[2] = start;
            }

            if events.contains(EventKind::PinchStart) {
                let payload = EventPayload::Pinch {
                    scale,
                    velocity,
                    value: start,
                };
                if let Some(control) = &self.control {
                    control.dispatch_event(id, EventKind::PinchStart, payload);
                }
            }
        }

        if matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        ) && let Some(n) = node
        {
            let start = self.slot(id).map_or(1.0, |s| s.gesture_start[2]);
            self.animator.set(n, start * scale);
            self.apply_animations();
        }

        if matches!(
            state,
            UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled
        ) && events.contains(EventKind::PinchEnd)
        {
            let value = node.map(|n| self.animator.value(n)).unwrap_or(1.0);
            let payload = EventPayload::Pinch {
                scale,
                velocity,
                value,
            };
            if let Some(control) = &self.control {
                control.dispatch_event(id, EventKind::PinchEnd, payload);
            }
        }
    }

    pub(super) fn sync_refresh(&mut self, id: NodeId) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let Native::Scroll(_scroll) = &slot.native else {
            return;
        };

        let wants_refresh =
            slot.events.contains(EventKind::Refresh) && slot.refresh_indicator.is_none();
        let has_control = slot.refresh_control.is_some();

        if wants_refresh && !has_control {
            let mtm = self.mtm;
            let relay = self.relay.clone();

            let control = UIRefreshControl::new(mtm);
            unsafe {
                control.addTarget_action_forControlEvents(
                    Some(&relay),
                    sel!(handleRefresh:),
                    objc2_ui_kit::UIControlEvents::ValueChanged,
                );
            }

            if let Some(slot) = self.slot_mut(id)
                && let Native::Scroll(scroll) = &slot.native
            {
                scroll.setRefreshControl(Some(&control));
                slot.refresh_control = Some(control);
            }
        } else if !wants_refresh {
            if has_control
                && let Some(slot) = self.slot_mut(id)
                && let Native::Scroll(scroll) = &slot.native
            {
                scroll.setRefreshControl(None);
                slot.refresh_control = None;
            }
            if let Some(slot) = self.slot_mut(id) {
                slot.refreshing = false;
            }
        }
    }

    pub(super) fn set_refreshing(&mut self, id: NodeId, refreshing: bool) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        if slot.refreshing == refreshing {
            return;
        }
        slot.refreshing = refreshing;

        if let Some(control) = &slot.refresh_control {
            if refreshing {
                control.beginRefreshing();
            } else {
                control.endRefreshing();
            }
        }
    }

    pub(super) fn handle_rotation(
        &mut self,
        id: NodeId,
        state: UIGestureRecognizerState,
        angle: f32,
        velocity: f32,
    ) {
        let Some(slot) = self.slot(id) else { return };
        let node = slot.rotation_node;
        let events = slot.events;

        if state == UIGestureRecognizerState::Began {
            let start = node.map(|n| self.animator.value(n)).unwrap_or(0.0);
            if let Some(slot) = self.slot_mut(id) {
                slot.gesture_start[3] = start;
            }

            if events.contains(EventKind::RotationStart) {
                let payload = EventPayload::Rotation {
                    angle,
                    velocity,
                    value: start,
                };
                if let Some(control) = &self.control {
                    control.dispatch_event(id, EventKind::RotationStart, payload);
                }
            }
        }

        if matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        ) && let Some(n) = node
        {
            let start = self.slot(id).map_or(0.0, |s| s.gesture_start[3]);
            self.animator.set(n, start + angle);
            self.apply_animations();
        }

        if matches!(
            state,
            UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled
        ) && events.contains(EventKind::RotationEnd)
        {
            let value = node.map(|n| self.animator.value(n)).unwrap_or(0.0);
            let payload = EventPayload::Rotation {
                angle,
                velocity,
                value,
            };
            if let Some(control) = &self.control {
                control.dispatch_event(id, EventKind::RotationEnd, payload);
            }
        }
    }
}
