//! Gesture, scroll, input and display-link callbacks.

use std::cell::RefCell;

use rustc_hash::FxHashMap as HashMap;

use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Point, Points};
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::EventPayload;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{
    UIGestureRecognizer, UIGestureRecognizerDelegate, UIGestureRecognizerState,
    UIHoverGestureRecognizer, UILongPressGestureRecognizer, UIPanGestureRecognizer,
    UIPinchGestureRecognizer, UIRefreshControl, UIRotationGestureRecognizer, UIScrollView,
    UIScrollViewDelegate, UITapGestureRecognizer, UITextField, UITextFieldDelegate, UITextView,
    UITextViewDelegate, UITouch, UIView,
};

use crate::input::Field;

use super::*;

/// Gesture target shared by every press and hover recognizer, and the
/// scroll views' delegate. The view's tag is the node id.
pub(crate) struct RelayIvars {
    pub(super) mounter: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<Mounter>>>,
    /// Engine content size by node id. Not the narrowed native `contentSize`.
    pub(super) content: RefCell<HashMap<NodeId, (f32, f32)>>,
    /// The display link driving animations; paused while none run.
    pub(super) link: RefCell<Option<Retained<CADisplayLink>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RelayIvars]
    pub(crate) struct Relay;

    impl Relay {
        #[unsafe(method(pressed:))]
        fn pressed(&self, gesture: &UILongPressGestureRecognizer) {
            let Some(view) = gesture.view() else { return };
            // SAFETY: the touch, in the view's coordinates.
            let point: CGPoint = unsafe { msg_send![gesture, locationInView: &*view] };
            self.forward_press(&view, gesture.state(), point);
        }

        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            let Some(shared) = self.ivars().mounter.borrow().upgrade() else {
                return;
            };
            shared.borrow_mut().tick_animation(link.targetTimestamp());
        }

        #[unsafe(method(hovered:))]
        fn hovered(&self, gesture: &UIHoverGestureRecognizer) {
            self.forward(gesture.view().as_deref(), gesture.state());
        }

        #[unsafe(method(handlePan:))]
        fn handle_pan(&self, gesture: &UIPanGestureRecognizer) {
            let Some(view) = gesture.view() else { return };
            let Some(superview) = view.superview() else { return };
            let translation = gesture.translationInView(Some(&superview));
            let velocity = gesture.velocityInView(Some(&superview));
            let id = view.tag() as NodeId;
            let state = gesture.state();
            with_mounter(move |m| {
                m.handle_pan(id, state, translation, velocity);
            });
        }

        #[unsafe(method(handlePinch:))]
        fn handle_pinch(&self, gesture: &UIPinchGestureRecognizer) {
            let Some(view) = gesture.view() else { return };
            let id = view.tag() as NodeId;
            let state = gesture.state();
            let scale = gesture.scale() as f32;
            let velocity = gesture.velocity() as f32;
            with_mounter(move |m| {
                m.handle_pinch(id, state, scale, velocity);
            });
        }

        #[unsafe(method(handleRotation:))]
        fn handle_rotation(&self, gesture: &UIRotationGestureRecognizer) {
            let Some(view) = gesture.view() else { return };
            let id = view.tag() as NodeId;
            let state = gesture.state();
            let angle = gesture.rotation().to_degrees() as f32;
            let velocity = gesture.velocity().to_degrees() as f32;
            with_mounter(move |m| {
                m.handle_rotation(id, state, angle, velocity);
            });
        }

        #[unsafe(method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:))]
        fn should_recognize_simultaneously(
            &self,
            gesture: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> Bool {
            if let Some(mounter) = self.ivars().mounter.borrow().upgrade()
                && let Ok(mounter) = mounter.try_borrow()
                && (mounter.pan_shares_scroll(gesture, other)
                    || mounter.pan_shares_scroll(other, gesture))
            {
                return true.into();
            }
            // A press (duration 0) must not take the pan away from a scroll
            // view. nsh's press recognizer never prevents a scroll pan.
            let press_and_scroll = |a: &UIGestureRecognizer, b: &UIGestureRecognizer| {
                a.isKindOfClass(UILongPressGestureRecognizer::class())
                    && b.isKindOfClass(UIPanGestureRecognizer::class())
                    && b.view()
                        .is_some_and(|view| view.isKindOfClass(UIScrollView::class()))
            };
            if press_and_scroll(gesture, other) || press_and_scroll(other, gesture) {
                return true.into();
            }
            let transform = |recognizer: &UIGestureRecognizer| {
                recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                    || recognizer.isKindOfClass(UIPinchGestureRecognizer::class())
                    || recognizer.isKindOfClass(UIRotationGestureRecognizer::class())
            };
            match (gesture.view(), other.view()) {
                (Some(a), Some(b)) => {
                    (std::ptr::eq::<UIView>(&*a, &*b) && transform(gesture) && transform(other))
                        .into()
                }
                _ => false.into(),
            }
        }

        #[unsafe(method(edited:))]
        fn edited(&self, sender: &AnyObject) {
            let Some(field) = sender.downcast_ref::<Field>() else {
                return;
            };
            let id = field.tag() as u32;
            let marked = field.composed();
            with_mounter(move |mounter| mounter.input_edited(id, marked));
        }

        #[unsafe(method(editingBegan:))]
        fn editing_began(&self, sender: &AnyObject) {
            if let Some(view) = sender.downcast_ref::<UIView>() {
                self.focus_edit(view, true);
            }
        }

        #[unsafe(method(editingEnded:))]
        fn editing_ended(&self, sender: &AnyObject) {
            if let Some(view) = sender.downcast_ref::<UIView>() {
                self.focus_edit(view, false);
            }
        }

        #[unsafe(method(dismissKeyboard:))]
        fn dismiss_keyboard(&self, _gesture: &UITapGestureRecognizer) {
            let Some(shared) = self.ivars().mounter.borrow().upgrade() else {
                return;
            };
            resign_keyboard(&shared);
        }

        #[unsafe(method(handleRefresh:))]
        fn handle_refresh(&self, control: &UIRefreshControl) {
            if let Some(scroll) = control.superview() {
                let id = scroll.tag() as NodeId;
                with_mounter(move |mounter| {
                    if let Some(c) = mounter.control.as_ref() {
                        c.dispatch_event(id, EventKind::Refresh, EventPayload::None);
                    }
                });
            }
        }

        #[unsafe(method(returned:))]
        fn returned(&self, sender: &AnyObject) {
            let Some(field) = sender.downcast_ref::<Field>() else {
                return;
            };
            if !field.resigns_on_return() {
                return;
            }
            // SAFETY: UITextField resigns first responder. The editing-ended
            // callback defers if a borrow is still live.
            let _: bool = unsafe { msg_send![field, resignFirstResponder] };
        }
    }

    unsafe impl UITextFieldDelegate for Relay {
        #[unsafe(method(textFieldDidChangeSelection:))]
        fn text_field_selection(&self, field: &UITextField) {
            self.input_selection(field.as_ref());
        }
    }

    unsafe impl UITextViewDelegate for Relay {
        #[unsafe(method(textViewDidChange:))]
        fn text_view_changed(&self, view: &UITextView) {
            let id = view.tag() as u32;
            let view: &UIView = view;
            let marked = crate::input::marked_text(view);
            with_mounter(move |mounter| mounter.input_edited(id, marked));
        }

        #[unsafe(method(textViewDidChangeSelection:))]
        fn text_view_selection(&self, view: &UITextView) {
            self.input_selection(view.as_ref());
        }

        #[unsafe(method(textViewDidBeginEditing:))]
        fn text_view_began(&self, view: &UITextView) {
            self.focus_edit(view.as_ref(), true);
        }

        #[unsafe(method(textViewDidEndEditing:))]
        fn text_view_ended(&self, view: &UITextView) {
            // A composition does not outlive focus: what was composed stays.
            let _: () = unsafe { msg_send![view, unmarkText] };
            let id = view.tag() as u32;
            with_mounter(move |mounter| {
                if let Some(change) = mounter.compose(id, None) {
                    mounter.report_composition(id, change);
                }
            });
            self.focus_edit(view.as_ref(), false);
        }
    }

    unsafe impl UIScrollViewDelegate for Relay {
        #[unsafe(method(scrollViewDidScroll:))]
        fn scroll_view_did_scroll(&self, scroll: &UIScrollView) {
            let offset = scroll.contentOffset();
            let id = scroll.tag() as NodeId;

            if let Some(shared) = self.ivars().mounter.borrow().upgrade()
                && let Ok(mounter) = shared.try_borrow()
            {
                let clamped = mounter.pan_scroll_offset(scroll, offset);
                if clamped != offset {
                    scroll.setContentOffset(clamped);
                    return;
                }
            }

            // Always update core's scroll position to keep tree() offset in sync
            if let Some(shared) = self.ivars().mounter.borrow().upgrade()
                && let Ok(mounter) = shared.try_borrow()
                && let Some(control) = &mounter.control
            {
                control.update_scroll_position(
                    id,
                    Point {
                        x: Points(offset.x as f32),
                        y: Points(offset.y as f32),
                    },
                );
            }

            if let Some(shared) = self.ivars().mounter.borrow().upgrade()
                && let Ok(mounter) = shared.try_borrow()
            {
                for (_, area) in &mounter.areas {
                    area.scrolled();
                }
            }
            self.scroll_event(scroll, EventKind::Scroll);
        }

        #[unsafe(method(scrollViewWillBeginDragging:))]
        fn scroll_view_will_begin_dragging(&self, scroll: &UIScrollView) {
            // Caught while gliding: the glide ended.
            if scroll.isDecelerating() {
                self.scroll_event(scroll, EventKind::MomentumEnd);
            }
            let dismiss = self
                .ivars()
                .mounter
                .borrow()
                .upgrade()
                .is_some_and(|shared| {
                    shared
                        .try_borrow()
                        .is_ok_and(|mounter| mounter.drag_dismisses(scroll))
                });
            if dismiss && let Some(shared) = self.ivars().mounter.borrow().upgrade() {
                resign_keyboard(&shared);
            }
            self.scroll_event(scroll, EventKind::ScrollDragStart);
        }

        #[unsafe(method(scrollViewDidEndDragging:willDecelerate:))]
        fn scroll_view_did_end_dragging(&self, scroll: &UIScrollView, decelerate: bool) {
            self.scroll_event(scroll, EventKind::ScrollDragEnd);
            self.scroll_event(
                scroll,
                if decelerate {
                    EventKind::MomentumStart
                } else {
                    EventKind::ScrollEnd
                },
            );
        }

        #[unsafe(method(scrollViewDidEndDecelerating:))]
        fn scroll_view_did_end_decelerating(&self, scroll: &UIScrollView) {
            self.scroll_event(scroll, EventKind::MomentumEnd);
            self.scroll_event(scroll, EventKind::ScrollEnd);
        }

        /// An animated `scrollTo` came to rest. A drag uses the decelerating
        /// callbacks instead.
        #[unsafe(method(scrollViewDidEndScrollingAnimation:))]
        fn scroll_view_did_end_scrolling_animation(&self, scroll: &UIScrollView) {
            self.scroll_event(scroll, EventKind::ScrollEnd);
        }

        #[unsafe(method(scrollViewWillEndDragging:withVelocity:targetContentOffset:))]
        fn scroll_view_will_end_dragging(
            &self,
            scroll: &UIScrollView,
            velocity: CGPoint,
            target_offset: *mut CGPoint,
        ) {
            let id = scroll.tag() as NodeId;
            let Some(shared) = self.ivars().mounter.borrow().upgrade() else {
                return;
            };
            let Ok(mounter) = shared.try_borrow() else {
                return;
            };
            let Some(slot) = mounter.slot(id) else {
                return;
            };

            // Build snap behavior from stored configuration
            let behavior = nlayer_core::snap::SnapBehavior {
                paging: slot.snap_paging,
                snap_interval: Points(slot.snap_interval.unwrap_or(0.0)),
                snap_offsets: slot
                    .snap_offsets
                    .as_ref()
                    .map(|offsets| offsets.iter().map(|&x| Points(x)).collect())
                    .unwrap_or_default(),
                snap_alignment: match slot.snap_alignment {
                    1 => nlayer_core::snap::SnapAlignment::Center,
                    2 => nlayer_core::snap::SnapAlignment::End,
                    _ => nlayer_core::snap::SnapAlignment::Start,
                },
                deceleration: slot.snap_deceleration,
                one_step: slot.snap_one_step,
            };

            // Get current state
            let current_offset = scroll.contentOffset();
            let bounds = scroll.bounds();
            let content_size = scroll.contentSize();
            let horizontal = scroll.alwaysBounceHorizontal();

            // SAFETY: targetContentOffset is a valid mutable pointer from UIKit
            let target = unsafe { &*target_offset };

            // Determine which axis to snap
            let (offset, projected, vel, viewport, content) = if horizontal {
                (
                    Points(current_offset.x as f32),
                    Points(target.x as f32),
                    velocity.x as f32 / 1000.0, // Convert points/second to points/millisecond
                    Points(bounds.size.width as f32),
                    Points(content_size.width as f32),
                )
            } else {
                (
                    Points(current_offset.y as f32),
                    Points(target.y as f32),
                    velocity.y as f32 / 1000.0,
                    Points(bounds.size.height as f32),
                    Points(content_size.height as f32),
                )
            };

            // Compute snap target
            if let Some(snap_offset) =
                nlayer_core::snap::snap_target(&behavior, offset, projected, vel, viewport, content)
            {
                // SAFETY: targetContentOffset is a valid mutable pointer from UIKit
                unsafe {
                    if horizontal {
                        (*target_offset).x = snap_offset.0 as CGFloat;
                    } else {
                        (*target_offset).y = snap_offset.0 as CGFloat;
                    }
                }
            }
        }
    }

    unsafe impl UIGestureRecognizerDelegate for Relay {
        #[unsafe(method(gestureRecognizerShouldBegin:))]
        fn should_begin(&self, recognizer: &UIGestureRecognizer) -> Bool {
            if recognizer.isKindOfClass(UIPanGestureRecognizer::class()) {
                let Some(view) = recognizer.view() else {
                    return true.into();
                };
                let velocity = recognizer
                    .downcast_ref::<UIPanGestureRecognizer>()
                    .map_or(CGPoint::ZERO, |pan| pan.velocityInView(Some(&view)));
                let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
                    return true.into();
                };
                let Ok(mounter) = mounter.try_borrow() else {
                    return true.into();
                };
                return mounter.pan_should_begin(view.tag() as NodeId, velocity).into();
            }
            if !recognizer.isKindOfClass(UILongPressGestureRecognizer::class()) {
                return true.into();
            }
            let Some(view) = recognizer.view() else {
                return true.into();
            };
            let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
                return true.into();
            };
            let Ok(mounter) = mounter.try_borrow() else {
                return true.into();
            };
            mounter.press_allowed(&view).into()
        }

        /// The keyboard tap only sees taps that should hide the keyboard.
        #[unsafe(method(gestureRecognizer:shouldReceiveTouch:))]
        fn should_receive_touch(
            &self,
            recognizer: &UIGestureRecognizer,
            touch: &UITouch,
        ) -> Bool {
            if recognizer.isKindOfClass(UIPanGestureRecognizer::class()) {
                if let (Some(view), Some(touched)) = (recognizer.view(), touch.view())
                    && let Some(mounter) = self.ivars().mounter.borrow().upgrade()
                    && let Ok(mut mounter) = mounter.try_borrow_mut()
                {
                    mounter.pan_touch(view.tag() as NodeId, &touched);
                }
                return true.into();
            }
            if !recognizer.isKindOfClass(UITapGestureRecognizer::class()) {
                return true.into();
            }
            let Some(view) = touch.view() else {
                return false.into();
            };
            let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
                return false.into();
            };
            let Ok(mounter) = mounter.try_borrow() else {
                return false.into();
            };
            // SAFETY: the touch, in the view's coordinates.
            let point: CGPoint = unsafe { msg_send![touch, locationInView: &*view] };
            mounter.tap_dismisses(&view, point).into()
        }
    }

    unsafe impl NSObjectProtocol for Relay {}
);

impl Relay {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RelayIvars {
            mounter: std::cell::RefCell::new(std::rc::Weak::new()),
            content: RefCell::new(Default::default()),
            link: RefCell::new(None),
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }

    pub(super) fn bind(&self, mounter: std::rc::Weak<std::cell::RefCell<Mounter>>) {
        *self.ivars().mounter.borrow_mut() = mounter;
    }

    pub(super) fn forward_press(
        &self,
        view: &UIView,
        state: UIGestureRecognizerState,
        point: CGPoint,
    ) {
        let id = view.tag() as u32;
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        mounter.borrow_mut().pressed(id, state, point);
    }

    pub(super) fn forward(&self, view: Option<&UIView>, state: UIGestureRecognizerState) {
        let Some(view) = view else { return };
        let id = view.tag() as u32;
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        mounter.borrow_mut().hovered(id, state);
    }

    /// A selection callback of an input: reports the range and any
    /// composition change (an input method unmarks by moving the caret).
    pub(super) fn input_selection(&self, view: &UIView) {
        let id = view.tag() as u32;
        let selection = crate::input::selection(view);
        let marked = crate::input::marked_text(view);
        with_mounter(move |mounter| mounter.input_selection(id, selection, marked));
    }

    pub(super) fn focus_edit(&self, view: &UIView, on: bool) {
        let id = view.tag() as u32;
        // `endEditing:` calls this while a dismiss still holds the mounter.
        with_mounter(move |mounter| {
            if on {
                mounter.begin_focus(id);
            } else {
                mounter.end_focus(id);
            }
        });
    }

    /// A scroll callback. Setting `contentOffset` while a batch is applied
    /// re-enters this; that report waits until the apply returns. `onScroll`
    /// is coalesced to the latest offset, once per main-queue turn.
    pub(super) fn scroll_event(&self, scroll: &UIScrollView, event: EventKind) {
        let id = scroll.tag() as NodeId;
        let content = self.ivars().content.borrow().get(&id).copied();
        let payload = scroll_metrics(scroll, content);
        let Some(shared) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        if let Ok(mounter) = shared.try_borrow() {
            mounter.dispatch_scroll(id, event, payload);
            return;
        }
        DispatchQueue::main().exec_async(move || {
            let Some(mounter) = current() else {
                return;
            };
            mounter.borrow().dispatch_scroll(id, event, payload);
        });
    }
}
