//! Applies one mount batch to UIKit views.
//!
//! Extracted from `nsh-platform-ios/src/mount.rs` (`set_frame`, the transform
//! matrix, insert-below-sibling), `decor.rs` (the clipping image layer) and
//! `input.rs` (the single-line field). Animation and gestures beyond press
//! and hover are not applied. A scroll view's delegate reports the scroll
//! events with the content offset, the engine's content size and the view's
//! size. A vertical scroll may narrow `contentSize` when nothing inside
//! reaches past the viewport; that native width is not what the event or
//! `tree()` reports.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Bounds, Point, Points, Size};
use nlayer_core::style::{BorderStyle, Color, TextStyle, TransformOrigin, ViewStyle};
use nlayer_core::{
    EventKind, EventMask, ImageSource, Interaction, KeyboardDismiss, KeyboardTaps, MountBatch,
    MountItem, NodeId, ROOT_ID, ResizeMode, ViewKind,
};
use nlayer_engine::{EngineControl, EventPayload};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_foundation::NSString;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{
    CADisplayLink, CAFrameRateRange, CALayer, CAShapeLayer, CATransaction, CATransform3D,
    kCAGravityResize,
};
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIAccessibilityTraitAdjustable, UIAccessibilityTraitButton,
    UIAccessibilityTraitHeader, UIAccessibilityTraitImage, UIAccessibilityTraitKeyboardKey,
    UIAccessibilityTraitLink, UIAccessibilityTraitNone, UIAccessibilityTraitNotEnabled,
    UIAccessibilityTraitSearchField, UIAccessibilityTraitSelected, UIAccessibilityTraitStaticText,
    UIAccessibilityTraitSummaryElement, UIAccessibilityTraitUpdatesFrequently, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer,
    UILongPressGestureRecognizer, UIScreen, UIScrollView,
    UIScrollViewContentInsetAdjustmentBehavior, UIScrollViewDelegate, UITapGestureRecognizer,
    UITextField, UITextFieldDelegate, UITextView, UITextViewDelegate, UITouch, UIView,
};

use crate::input::{AreaInput, Field};
use crate::text_view::TextView;

/// One native view and the paint the engine last set on it.
struct Slot {
    native: Native,
    style: ViewStyle,
    /// `activeStyle`, `hoverStyle`, `focusStyle`. `None` clears that paint.
    states: [Option<Paint>; 3],
    pressed: bool,
    hovered: bool,
    /// This view, or a field inside it, has focus. `focusStyle` paints then.
    focused: bool,
    press: Option<Retained<UILongPressGestureRecognizer>>,
    /// The view itself listens to `onPress` (not only a span inside it).
    own_press: bool,
    /// Listeners, including the scroll events. A scroll callback fires only
    /// a kind this contains.
    events: EventMask,
    /// A nested span listens to `onPress`.
    link_press: bool,
    hover: Option<Retained<UIHoverGestureRecognizer>>,
    /// A dashed or dotted uniform border, above the children.
    dash: Option<Retained<CAShapeLayer>>,
    /// Clip shape when the four radii differ. Reused across paints.
    mask: Option<Retained<CAShapeLayer>>,
    /// Per-side borders when the four sides are not one width and color.
    sides: [Option<Retained<CALayer>>; 4],
    /// The fitted bitmap, present only for an image view.
    picture: Option<Picture>,
    /// `testID`. Not drawn, and not an accessibility id.
    test_id: Option<Arc<str>>,
    /// `keyboardShouldPersistTaps`.
    taps: KeyboardTaps,
    /// `keyboardDismissMode`.
    dismiss: KeyboardDismiss,
    /// The input's content box, to reapply when the editor is replaced.
    input_content: Option<Bounds<Points>>,
    /// Tap that resigns the keyboard. Only a scroll has one.
    keyboard_tap: Option<Retained<UITapGestureRecognizer>>,
    /// `role`, label, hint, state, value and whether the subtree is hidden
    /// from assistive technologies.
    accessibility: nlayer_core::Accessibility,
    /// Visual properties driven by animation nodes.
    bindings: Vec<(nlayer_core::animation::AnimationProp, u32)>,
}

struct Paint {
    view: ViewStyle,
    text: TextStyle,
}

enum Native {
    View(Retained<UIView>),
    Text(Retained<TextView>),
    Image(Retained<UIView>),
    Scroll(Retained<UIScrollView>),
    Input(Retained<Field>),
    Area(AreaInput),
}

impl Native {
    fn view(&self) -> &UIView {
        match self {
            Native::View(view) | Native::Image(view) => view,
            Native::Text(view) => view,
            Native::Scroll(view) => view,
            Native::Input(view) => view,
            Native::Area(area) => area.view(),
        }
    }
}

/// Gesture target shared by every press and hover recognizer, and the
/// scroll views' delegate. The view's tag is the node id.
struct RelayIvars {
    mounter: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<Mounter>>>,
    /// Engine content size by node id. Not the narrowed native `contentSize`.
    content: RefCell<HashMap<NodeId, (f32, f32)>>,
    /// The display link driving animations; paused while none run.
    link: RefCell<Option<Retained<CADisplayLink>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RelayIvars]
    struct Relay;

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
            if dismiss {
                if let Some(shared) = self.ivars().mounter.borrow().upgrade() {
                    resign_keyboard(&shared);
                }
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
    }

    unsafe impl UIGestureRecognizerDelegate for Relay {
        #[unsafe(method(gestureRecognizerShouldBegin:))]
        fn should_begin(&self, recognizer: &UIGestureRecognizer) -> Bool {
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
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RelayIvars {
            mounter: std::cell::RefCell::new(std::rc::Weak::new()),
            content: RefCell::new(HashMap::new()),
            link: RefCell::new(None),
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }

    fn bind(&self, mounter: std::rc::Weak<std::cell::RefCell<Mounter>>) {
        *self.ivars().mounter.borrow_mut() = mounter;
    }

    fn forward_press(&self, view: &UIView, state: UIGestureRecognizerState, point: CGPoint) {
        let id = view.tag() as u32;
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        mounter.borrow_mut().pressed(id, state, point);
    }

    fn forward(&self, view: Option<&UIView>, state: UIGestureRecognizerState) {
        let Some(view) = view else { return };
        let id = view.tag() as u32;
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        mounter.borrow_mut().hovered(id, state);
    }

    /// A selection callback of an input: reports the range and any
    /// composition change (an input method unmarks by moving the caret).
    fn input_selection(&self, view: &UIView) {
        let id = view.tag() as u32;
        let selection = crate::input::selection(view);
        let marked = crate::input::marked_text(view);
        with_mounter(move |mounter| mounter.input_selection(id, selection, marked));
    }

    fn focus_edit(&self, view: &UIView, on: bool) {
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
    fn scroll_event(&self, scroll: &UIScrollView, event: EventKind) {
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

/// Reports `a` on `view`: element, label, hint, value, traits and whether
/// the subtree is hidden.
fn apply_accessibility(view: &UIView, a: &nlayer_core::Accessibility, mtm: MainThreadMarker) {
    use nlayer_core::AccessibilityRole;
    view.setIsAccessibilityElement(a.is_element(), mtm);
    let label = a.label.as_ref().map(|label| NSString::from_str(label));
    view.setAccessibilityLabel(label.as_deref(), mtm);
    let hint = a.hint.as_ref().map(|hint| NSString::from_str(hint));
    view.setAccessibilityHint(hint.as_deref(), mtm);
    let value = spoken_value(a).map(|value| NSString::from_str(&value));
    view.setAccessibilityValue(value.as_deref(), mtm);
    view.setAccessibilityElementsHidden(a.hidden, mtm);
    // SAFETY: immutable UIKit constants.
    let traits = unsafe {
        let mut traits = match a.role {
            AccessibilityRole::Button
            | AccessibilityRole::ImageButton
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Radio
            | AccessibilityRole::Switch
            | AccessibilityRole::Tab
            | AccessibilityRole::Menuitem
            | AccessibilityRole::MenuItemCheckbox
            | AccessibilityRole::MenuItemRadio
            | AccessibilityRole::TreeItem
            | AccessibilityRole::Option => UIAccessibilityTraitButton,
            AccessibilityRole::Link => UIAccessibilityTraitLink,
            AccessibilityRole::Search => UIAccessibilityTraitSearchField,
            AccessibilityRole::Image => UIAccessibilityTraitImage,
            AccessibilityRole::KeyboardKey => UIAccessibilityTraitKeyboardKey,
            AccessibilityRole::Text => UIAccessibilityTraitStaticText,
            AccessibilityRole::Adjustable | AccessibilityRole::Spinbutton => {
                UIAccessibilityTraitAdjustable
            }
            AccessibilityRole::Header | AccessibilityRole::ColumnHeader => {
                UIAccessibilityTraitHeader
            }
            AccessibilityRole::Summary => UIAccessibilityTraitSummaryElement,
            AccessibilityRole::Progressbar | AccessibilityRole::Timer => {
                UIAccessibilityTraitUpdatesFrequently
            }
            _ => UIAccessibilityTraitNone,
        };
        if a.role == AccessibilityRole::ImageButton {
            traits |= UIAccessibilityTraitImage;
        }
        if a.state.disabled {
            traits |= UIAccessibilityTraitNotEnabled;
        }
        if a.state.selected || a.state.checked == Some(nlayer_core::Checked::True) {
            traits |= UIAccessibilityTraitSelected;
        }
        traits
    };
    view.setAccessibilityTraits(traits, mtm);
}

/// What VoiceOver reads: the value (as a percentage of its range, or the
/// text), then checked / expanded.
fn spoken_value(a: &nlayer_core::Accessibility) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(v) = &a.value {
        if let Some(text) = &v.text {
            parts.push(text.to_string());
        } else if let Some(now) = v.now {
            match (v.min, v.max) {
                (Some(min), Some(max)) if max > min => {
                    parts.push(format!(
                        "{}%",
                        (((now - min) / (max - min)) * 100.0).round()
                    ));
                }
                _ => parts.push(format!("{now}")),
            }
        }
    }
    match a.state.checked {
        Some(nlayer_core::Checked::True) => parts.push("checked".into()),
        Some(nlayer_core::Checked::False) => parts.push("not checked".into()),
        Some(nlayer_core::Checked::Mixed) => parts.push("mixed".into()),
        None => {}
    }
    match a.state.expanded {
        Some(true) => parts.push("expanded".into()),
        Some(false) => parts.push("collapsed".into()),
        None => {}
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn scroll_metrics(scroll: &UIScrollView, content: Option<(f32, f32)>) -> EventPayload {
    let offset = scroll.contentOffset();
    let native = scroll.contentSize();
    let (content_w, content_h) = content.unwrap_or((native.width as f32, native.height as f32));
    let size = scroll.bounds().size;
    EventPayload::Scroll {
        x: offset.x as f32,
        y: offset.y as f32,
        content_w,
        content_h,
        width: size.width as f32,
        height: size.height as f32,
    }
}

/// Whether anything under `root` reaches past `limit` points from its left
/// edge, in `root`'s space. A scroll view inside scrolls on its own, so what
/// it holds does not count. Stops at the first view that reaches.
fn reaches_beyond(root: &UIView, limit: CGFloat) -> bool {
    let mut pending = Vec::new();
    push_subviews(root, &mut pending);
    while let Some(view) = pending.pop() {
        let frame = view.convertRect_toView(view.bounds(), Some(root));
        if frame.origin.x + frame.size.width > limit {
            return true;
        }
        if view.downcast_ref::<UIScrollView>().is_none() {
            push_subviews(&view, &mut pending);
        }
    }
    false
}

fn push_subviews(view: &UIView, out: &mut Vec<Retained<UIView>>) {
    let subs = view.subviews();
    for index in 0..subs.count() {
        out.push(subs.objectAtIndex(index));
    }
}

/// The views of one surface. Touched only on the main thread.
pub(crate) struct Mounter {
    mtm: MainThreadMarker,
    slots: Vec<Option<Slot>>,
    relay: Retained<Relay>,
    control: Option<EngineControl>,
    viewport: Size<Points>,
    insets: nlayer_core::geometry::Edges<Points>,
    /// Directory `asset:/` paths are read from (the directory of `app.js`).
    assets: PathBuf,
    /// The field that is editing, if one is.
    focused: Option<NodeId>,
    /// Latest `onScroll` per view, flushed once on the next main-queue turn.
    scrolls: RefCell<crate::scroll_frame::ScrollFrames<EventPayload>>,
    /// Set by `nlayer test`. Replaces the window's size, safe area and
    /// keyboard height. A pin with a keyboard height ignores the software keyboard.
    pin: Option<crate::pin::Pin>,
    /// Software keyboard overlap, in points. An explicit pin height replaces it.
    keyboard: Points,
    /// Vertical scrolls whose content size arrived in this batch, with the
    /// height that came with it. A size that looks too wide is judged only
    /// after the batch has put every child's frame in place.
    content_sizes: Vec<(NodeId, CGFloat)>,
    /// The engine's width and height for every vertical scroll the platform
    /// narrowed. The engine does not resend a content size it did not change,
    /// so a later batch that widens what such a scroll holds gives that width
    /// back from here.
    narrowed: Vec<(NodeId, CGFloat, CGFloat)>,
    /// Whether this batch moved any frame. Nothing under a narrowed scroll can
    /// have grown otherwise, so that judgement is skipped.
    frames_applied: bool,
    /// The animation graph, moved by the display link.
    animator: crate::animation::Animator,
}

impl Mounter {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        root: Retained<UIView>,
        assets: PathBuf,
    ) -> Rc<std::cell::RefCell<Self>> {
        let relay = Relay::new(mtm);
        let mut slots = Vec::new();
        slots.resize_with(ROOT_ID as usize + 1, || None);
        root.setTag(ROOT_ID as isize);
        slots[ROOT_ID as usize] = Some(Slot::plain(Native::View(root)));
        let mounter = Rc::new(std::cell::RefCell::new(Self {
            mtm,
            slots,
            relay: relay.clone(),
            control: None,
            viewport: Size::default(),
            insets: nlayer_core::geometry::Edges::default(),
            assets,
            focused: None,
            scrolls: RefCell::default(),
            pin: None,
            keyboard: Points::ZERO,
            content_sizes: Vec::new(),
            narrowed: Vec::new(),
            frames_applied: false,
            animator: crate::animation::Animator::default(),
        }));
        CURRENT.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&mounter)));
        relay.bind(Rc::downgrade(&mounter));
        // SAFETY: `tick:` is implemented by `Relay`, which outlives the
        // link (the link stays in the relay and is invalidated with it).
        unsafe {
            let link = CADisplayLink::displayLinkWithTarget_selector(&relay, sel!(tick:));
            link.setPaused(true);
            #[allow(deprecated)]
            let max = UIScreen::mainScreen(mtm).maximumFramesPerSecond().max(1) as f32;
            link.setPreferredFrameRateRange(CAFrameRateRange {
                minimum: max.min(80.0),
                maximum: max,
                preferred: max,
            });
            link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
            *relay.ivars().link.borrow_mut() = Some(link);
        }
        mounter
    }

    pub(crate) fn set_control(&mut self, control: EngineControl) {
        if self.pin.as_ref().is_none_or(|pin| pin.keyboard.is_none())
            && self.keyboard != Points::ZERO
        {
            control.set_keyboard_inset(self.keyboard);
        }
        self.control = Some(control);
    }

    /// The software keyboard's overlap changed. An explicit pin height wins.
    pub(crate) fn set_keyboard(&mut self, height: f32) {
        if self.pin.as_ref().is_some_and(|pin| pin.keyboard.is_some()) {
            return;
        }
        let height = Points(height);
        if self.keyboard == height {
            return;
        }
        self.keyboard = height;
        if let Some(control) = &self.control {
            control.set_keyboard_inset(height);
        }
    }

    /// The height `env(keyboard-inset-height)` is using. A pin replaces the
    /// live keyboard, including a pin of 0.
    fn keyboard_inset(&self) -> f32 {
        self.pin
            .as_ref()
            .and_then(|pin| pin.keyboard)
            .unwrap_or(self.keyboard.0)
    }

    /// Resigns the focused field. A missing window still resigns the field
    /// the mounter is tracking.
    fn end_editing(&self) {
        if let Some(window) = self.view(ROOT_ID).and_then(|view| view.window()) {
            // SAFETY: `endEditing:` walks the window's responders.
            let _: bool = unsafe { msg_send![&*window, endEditing: true] };
            return;
        }
        if let Some(id) = self.focused
            && let Some(view) = self.input_view(id)
        {
            let _: bool = unsafe { msg_send![&*view, resignFirstResponder] };
        }
    }

    fn press_allowed(&self, view: &UIView) -> bool {
        let (taps, on_input) = self.tap_context(view);
        nlayer_core::press_allowed(Points(self.keyboard_inset()), taps, on_input)
    }

    fn tap_dismisses(&self, view: &UIView, at: CGPoint) -> bool {
        let (taps, on_input) = self.tap_context(view);
        let handled = self.handled(view, at);
        nlayer_core::tap_dismisses(Points(self.keyboard_inset()), taps, on_input, handled)
    }

    fn drag_dismisses(&self, scroll: &UIScrollView) -> bool {
        let id = scroll.tag() as NodeId;
        let mode = self
            .slot(id)
            .filter(|slot| std::ptr::eq(slot.native.view(), scroll as &UIView))
            .map(|slot| slot.dismiss)
            .unwrap_or_default();
        nlayer_core::drag_dismisses(Points(self.keyboard_inset()), mode)
    }

    /// The nearest scroll's tap mode, and whether the walk to it hits an input.
    fn tap_context(&self, view: &UIView) -> (Option<KeyboardTaps>, bool) {
        let mut on_input = false;
        let mut current = Some(view.retain());
        while let Some(v) = current {
            let id = v.tag() as NodeId;
            if let Some(slot) = self
                .slot(id)
                .filter(|slot| std::ptr::eq(slot.native.view(), &*v))
            {
                if matches!(slot.native, Native::Input(_) | Native::Area(_)) {
                    on_input = true;
                }
                if matches!(slot.native, Native::Scroll(_)) {
                    return (Some(slot.taps), on_input);
                }
            }
            current = v.superview();
        }
        (None, on_input)
    }

    /// An `onPress` or an input from `view` through the nearest scroll.
    /// A pressable span counts only under `at`, which is in `view`'s coordinates.
    fn handled(&self, view: &UIView, at: CGPoint) -> bool {
        let mut current = Some(view.retain());
        let mut point = Some(at);
        while let Some(v) = current {
            let id = v.tag() as NodeId;
            if let Some(slot) = self
                .slot(id)
                .filter(|slot| std::ptr::eq(slot.native.view(), &*v))
            {
                let on_link = if let (Native::Text(text), Some(at)) = (&slot.native, point) {
                    text.link_at(at).is_some()
                } else {
                    false
                };
                if matches!(slot.native, Native::Input(_) | Native::Area(_))
                    || slot.own_press
                    || on_link
                {
                    return true;
                }
                if matches!(slot.native, Native::Scroll(_)) {
                    return false;
                }
            }
            point = None;
            current = v.superview();
        }
        false
    }

    /// The system's appearance changed: `platform.colorScheme()` follows it.
    /// A test pin replaces the system for the run.
    pub(crate) fn set_color_scheme(&mut self, dark: bool) {
        if self
            .pin
            .as_ref()
            .is_some_and(|pin| pin.color_scheme.is_some())
        {
            return;
        }
        if let Some(control) = &self.control {
            control.set_color_scheme(nlayer_core::ColorScheme::from_dark(dark));
        }
    }

    pub(crate) fn set_pin(&mut self, pin: crate::pin::Pin) {
        self.viewport = Size::new(Points(pin.width), Points(pin.height));
        self.insets = nlayer_core::geometry::Edges {
            top: Points(pin.safe_area.top),
            right: Points(pin.safe_area.right),
            bottom: Points(pin.safe_area.bottom),
            left: Points(pin.safe_area.left),
        };
        self.pin = Some(pin);
    }

    pub(crate) fn pin_size(&self) -> Option<(f32, f32)> {
        self.pin.as_ref().map(|pin| (pin.width, pin.height))
    }

    /// Tells the engine the surface size and safe area, when either changed.
    pub(crate) fn publish(&mut self, size: CGSize, insets: objc2_ui_kit::UIEdgeInsets) {
        let (size, insets) = if let Some(pin) = &self.pin {
            (
                CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
                objc2_ui_kit::UIEdgeInsets {
                    top: pin.safe_area.top as CGFloat,
                    left: pin.safe_area.left as CGFloat,
                    bottom: pin.safe_area.bottom as CGFloat,
                    right: pin.safe_area.right as CGFloat,
                },
            )
        } else {
            (size, insets)
        };
        let keyboard = self.pin.as_ref().and_then(|pin| pin.keyboard);
        let size = Size::new(Points(size.width as f32), Points(size.height as f32));
        let insets = nlayer_core::geometry::Edges {
            top: Points(insets.top as f32),
            right: Points(insets.right as f32),
            bottom: Points(insets.bottom as f32),
            left: Points(insets.left as f32),
        };
        let Some(control) = self.control.clone() else {
            self.viewport = size;
            self.insets = insets;
            return;
        };
        if size != self.viewport {
            self.viewport = size;
            control.resize(size);
        }
        if insets != self.insets {
            self.insets = insets;
            control.set_safe_area(insets);
        }
        if let Some(height) = keyboard {
            control.set_keyboard_inset(Points(height));
        }
    }

    pub(crate) fn view_of(&self, id: NodeId) -> Option<Retained<UIView>> {
        self.view(id).map(|view| view.retain())
    }

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
            scroll.setContentOffset_animated(CGPoint::new(x.0 as CGFloat, y.0 as CGFloat), true);
        }
        self.settle_content_sizes();
        self.apply_animations();
        self.report_finished();
    }

    /// Runs one display frame: the graph moves, bound values are painted,
    /// and finished animations go back to the engine.
    fn tick_animation(&mut self, now: f64) {
        if self.animator.tick(now) {
            self.apply_animations();
        }
        self.report_finished();
        self.update_link();
    }

    fn update_link(&mut self) {
        let animating = self.animator.animating();
        if let Some(link) = self.relay.ivars().link.borrow().as_ref() {
            link.setPaused(!animating);
        }
    }

    fn report_finished(&mut self) {
        let finished = self.animator.take_finished();
        if !finished.is_empty()
            && let Some(control) = &self.control
        {
            control.animations_finished(finished);
        }
    }

    /// Paints every bound property over the slot's static style.
    fn apply_animations(&mut self) {
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
                        style.background_color = nlayer_core::style::Color(value.number() as u32)
                    }
                    nlayer_core::animation::AnimationProp::BorderColor => {
                        style.borders.colors = [nlayer_core::style::Color(value.number() as u32); 4]
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
            }
        }
    }

    /// Drops a deleted scroll's engine size and any width still being judged.
    fn forget_scroll(&mut self, id: NodeId) {
        self.relay.ivars().content.borrow_mut().remove(&id);
        self.narrowed.retain(|&(n, ..)| n != id);
        self.content_sizes.retain(|&(n, _)| n != id);
    }

    /// Narrows a vertical scroll's native content width to its viewport when
    /// nothing inside reaches past it. Judged after the batch, so a child
    /// that arrived with the size counts. The engine size stays on the relay.
    fn settle_content_sizes(&mut self) {
        let frames_applied = std::mem::take(&mut self.frames_applied);
        for (id, height) in std::mem::take(&mut self.content_sizes) {
            let mut narrowed = None;
            if let Some(Native::Scroll(scroll)) = self.native(id) {
                let viewport = scroll.bounds().size.width;
                let content = scroll.contentSize().width;
                // Content that fits cannot be narrowed; walking the subtree
                // to learn that would cost the page on every batch.
                if viewport > 0.0 && content > viewport {
                    let width = crate::scroll_extent::settled_content_width(
                        viewport as f64,
                        content as f64,
                        reaches_beyond(scroll, viewport + 0.5),
                    );
                    if width != content as f64 {
                        scroll.setContentSize(CGSize::new(width as CGFloat, height));
                        narrowed = Some((content, height));
                    }
                }
            }
            self.narrowed.retain(|&(n, ..)| n != id);
            if let Some((width, height)) = narrowed {
                self.narrowed.push((id, width, height));
            }
        }
        if frames_applied {
            self.rejudge_narrowed();
        }
    }

    /// Gives the engine's width back once a later batch widens what a
    /// narrowed scroll holds. Without this the tail stays out of reach.
    fn rejudge_narrowed(&mut self) {
        let mut i = 0;
        while i < self.narrowed.len() {
            let (id, core, height) = self.narrowed[i];
            let mut done = true;
            if let Some(Native::Scroll(scroll)) = self.native(id)
                && crate::scroll_extent::still_judged(scroll.alwaysBounceHorizontal())
            {
                let viewport = scroll.bounds().size.width;
                let reaches = reaches_beyond(scroll, viewport + 0.5);
                let width =
                    crate::scroll_extent::narrowed_width(viewport as f64, core as f64, reaches);
                if width != scroll.contentSize().width as f64 {
                    scroll.setContentSize(CGSize::new(width as CGFloat, height));
                }
                done = width == core as f64;
            }
            if done {
                self.narrowed.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn apply_item(&mut self, item: MountItem) {
        match item {
            MountItem::Create { id, kind } => self.create(id, kind),
            MountItem::Insert {
                parent,
                child,
                before,
            } => self.insert(parent, child, before),
            MountItem::Remove { id } => {
                if let Some(view) = self.view(id) {
                    view.removeFromSuperview();
                }
            }
            MountItem::Delete { id } => {
                self.forget_scroll(id);
                if self.focused == Some(id) {
                    self.end_focus(id);
                }
                if let Some(slot) = self.take(id) {
                    slot.native.view().removeFromSuperview();
                }
            }
            MountItem::Frame { id, frame } => {
                self.frames_applied = true;
                if id == ROOT_ID {
                    return;
                }
                // Corners, borders and the image frame depend on the size,
                // not where the view sits. A move leaves the paint alone.
                let resized = self.view(id).is_some_and(|view| set_frame(view, frame));
                if resized {
                    self.repaint(id);
                }
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
                    .any(|link| link.events.contains(EventKind::Press));
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
            MountItem::ScrollContent { id, size } => {
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                let width = size.width.0;
                let height = size.height.0;
                self.relay
                    .ivars()
                    .content
                    .borrow_mut()
                    .insert(id, (width, height));
                scroll.setContentSize(CGSize::new(width as CGFloat, height as CGFloat));
                if crate::scroll_extent::still_judged(scroll.alwaysBounceHorizontal()) {
                    self.content_sizes.push((id, height as CGFloat));
                }
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
                        CGPoint::new(x.0 as CGFloat, y.0 as CGFloat),
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
                | nlayer_core::Command::ScrollToEnd { .. } => {
                    // The core resolves list commands to a `ScrollTo`.
                }
            },
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
            MountItem::Animation(op) => {
                self.animator.apply(op.clone());
                self.update_link();
            }
            MountItem::AnimationBindings { id, bindings } => {
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
        }
    }

    /// Replaces the editor when `multiline` changes. The new view takes the
    /// old one's place in its superview, text, focus and content box.
    fn set_input_multiline(&mut self, id: NodeId, multiline: bool) {
        let area = matches!(self.native(id), Some(Native::Area(_)));
        if area == multiline {
            return;
        }
        let Some(old) = self.input_view(id) else {
            return;
        };
        let content = self.slot(id).and_then(|slot| slot.input_content);
        let text = match self.native(id) {
            Some(Native::Input(field)) => field.shown(),
            Some(Native::Area(area)) => area.shown(),
            _ => return,
        };
        let focused = old.isFirstResponder();
        let relay = ProtocolObject::from_ref(&*self.relay);
        let native = if multiline {
            Native::Area(AreaInput::new(self.mtm, relay))
        } else {
            Native::Input(Field::new(self.mtm, ProtocolObject::from_ref(&*self.relay)))
        };
        let new = native.view().retain();
        new.setTag(id as isize);
        new.setOpaque(false);
        new.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        new.setFrame(old.frame());
        if let Some(parent) = old.superview() {
            let subs = parent.subviews();
            let index = (0..subs.count()).find(|&index| {
                Retained::as_ptr(&subs.objectAtIndex(index)) == Retained::as_ptr(&old)
            });
            if let Some(index) = index {
                parent.insertSubview_atIndex(&new, index as isize);
            }
        }
        old.removeFromSuperview();
        match &native {
            Native::Input(field) => field.set_value(&text, 0),
            Native::Area(area) => area.set_value(&text, 0),
            _ => {}
        }
        let accessibility = self.slot(id).map(|slot| slot.accessibility.clone());
        if let Some(accessibility) = &accessibility {
            apply_accessibility(&new, accessibility, self.mtm);
        }
        if let Some(slot) = self.slot_mut(id) {
            slot.native = native;
        }
        if let Some(content) = content {
            match self.native(id) {
                Some(Native::Input(field)) => field.set_content(content),
                Some(Native::Area(area)) => area.set_content(content),
                _ => {}
            }
        }
        self.repaint(id);
        if focused {
            new.becomeFirstResponder();
        }
    }

    fn create(&mut self, id: NodeId, kind: ViewKind) {
        let (native, picture) = match kind {
            ViewKind::View => (Native::View(UIView::new(self.mtm)), None),
            ViewKind::Text => (Native::Text(TextView::new(self.mtm)), None),
            ViewKind::Scroll => {
                let scroll =
                    UIScrollView::initWithFrame(UIScrollView::alloc(self.mtm), CGRect::ZERO);
                scroll.setContentInsetAdjustmentBehavior(
                    UIScrollViewContentInsetAdjustmentBehavior::Never,
                );
                scroll.setDelaysContentTouches(false);
                // SAFETY: `Relay` implements `UIScrollViewDelegate`. UIKit
                // holds the delegate weakly; the mounter keeps the relay.
                unsafe {
                    scroll.setDelegate(Some(ProtocolObject::from_ref(&*self.relay)));
                }
                (Native::Scroll(scroll), None)
            }
            ViewKind::Image => {
                let (view, picture) = image_layers(self.mtm);
                (Native::Image(view), Some(picture))
            }
            ViewKind::Input => (
                Native::Input(Field::new(self.mtm, ProtocolObject::from_ref(&*self.relay))),
                None,
            ),
        };
        let view = native.view().retain();
        view.setTag(id as isize);
        view.setOpaque(false);
        view.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        self.ensure(id);
        let mut slot = Slot::plain(native);
        slot.picture = picture;
        if kind == ViewKind::Scroll {
            slot.keyboard_tap = Some(keyboard_tap(self.mtm, &self.relay, &view));
        }
        self.slots[id as usize] = Some(slot);
    }

    fn insert(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        let Some(child_view) = self.view(child).map(|v| v.retain()) else {
            return;
        };
        let Some(parent_view) = self.view(parent) else {
            return;
        };
        match before.and_then(|id| self.view(id)).map(|v| v.retain()) {
            Some(before) => {
                parent_view.insertSubview_belowSubview(&child_view, &before);
            }
            None => parent_view.addSubview(&child_view),
        }
    }

    fn set_events(&mut self, id: NodeId, events: nlayer_core::EventMask) {
        if let Some(slot) = self.slot_mut(id) {
            slot.events = events;
            slot.own_press = events.contains(EventKind::Press);
        }
        self.sync_press(id);
        self.sync_hover(id);
    }

    fn dispatch_scroll(&self, id: NodeId, event: EventKind, payload: EventPayload) {
        if event == EventKind::Scroll {
            let arm = self.scrolls.borrow_mut().note(id, payload);
            if arm {
                DispatchQueue::main().exec_async(move || {
                    let Some(mounter) = current() else {
                        return;
                    };
                    mounter.borrow().flush_scrolls();
                });
            }
            return;
        }
        if let Some(pending) = self.scrolls.borrow_mut().take_id(id) {
            self.send_scroll(id, EventKind::Scroll, pending);
        }
        self.send_scroll(id, event, payload);
    }

    fn flush_scrolls(&self) {
        let pending = self.scrolls.borrow_mut().take();
        for (id, payload) in pending {
            self.send_scroll(id, EventKind::Scroll, payload);
        }
    }

    fn send_scroll(&self, id: NodeId, event: EventKind, payload: EventPayload) {
        let listens = self
            .slot(id)
            .is_some_and(|slot| slot.events.contains(event));
        // The core follows a list's offset even without `onScroll`: only
        // the other callbacks wait for a listener.
        if event != EventKind::Scroll && !listens {
            return;
        }
        let Some(control) = &self.control else {
            return;
        };
        control.dispatch_event(id, event, payload);
    }

    fn sync_press(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let want = slot.own_press || slot.link_press;
        let view = slot.native.view().retain();
        let input = matches!(slot.native, Native::Input(_) | Native::Area(_));
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
                gesture.setDelegate(Some(ProtocolObject::from_ref(&*relay)));
                if input {
                    gesture.setCancelsTouchesInView(false);
                }
                view.addGestureRecognizer(&gesture);
                slot.press = Some(gesture);
            }
            (Some(gesture), false) => {
                view.removeGestureRecognizer(gesture);
                slot.press = None;
                slot.pressed = false;
            }
            _ => {}
        }
    }

    fn sync_hover(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let view = slot.native.view().retain();
        let want_hover = slot.states[state_index(Interaction::Hover)].is_some();
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
            }
            _ => {}
        }
    }

    fn pressed(&mut self, id: NodeId, state: UIGestureRecognizerState, point: CGPoint) {
        if state == UIGestureRecognizerState::Began && self.press_blocked(id, point) {
            return;
        }
        let on = matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        );
        let ended = state == UIGestureRecognizerState::Ended;
        if let Some(slot) = self.slot_mut(id) {
            slot.pressed = on;
        }
        self.repaint(id);
        if !ended {
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

    /// A disabled view is under `point`, from the recognizer's view down:
    /// the press stops there and does not reach an ancestor.
    fn press_blocked(&self, id: NodeId, point: CGPoint) -> bool {
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

    fn link_target(&self, id: NodeId, point: CGPoint) -> Option<NodeId> {
        let Native::Text(view) = self.native(id)? else {
            return None;
        };
        view.link_at(point)
    }

    fn hovered(&mut self, id: NodeId, state: UIGestureRecognizerState) {
        let on = matches!(
            state,
            UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed
        );
        if let Some(slot) = self.slot_mut(id) {
            slot.hovered = on;
        }
        self.repaint(id);
    }

    fn repaint(&mut self, id: NodeId) {
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

    fn report_input(&self, id: NodeId, text: String) {
        if let Some(control) = &self.control {
            control.input_changed(id, text);
        }
    }

    /// The editor reported an edit with `marked` (its composition, if any).
    /// A composition that starts or changes is reported before the text
    /// holding it, and one that ends after the text it committed, as
    /// browsers order them.
    fn input_edited(&mut self, id: NodeId, marked: Option<String>) {
        let update = match self.native(id) {
            Some(Native::Input(field)) => field.edited(),
            Some(Native::Area(area)) => area.edited(),
            _ => return,
        };
        let change = if marked.is_some() {
            self.compose(id, marked.clone())
        } else {
            None
        };
        if let Some(change) = change {
            self.report_composition(id, change);
        }
        if let Some(crate::input::TextUpdate::Report(text)) = update {
            self.report_input(id, text);
        }
        if marked.is_none()
            && let Some(change) = self.compose(id, None)
        {
            self.report_composition(id, change);
        }
    }

    /// The input's selection moved (and its composition may have ended).
    fn input_selection(
        &mut self,
        id: NodeId,
        selection: Option<(u32, u32)>,
        marked: Option<String>,
    ) {
        let selected = match self.native(id) {
            Some(Native::Input(field)) => {
                selection.and_then(|(start, end)| field.selected(start, end))
            }
            Some(Native::Area(area)) => {
                selection.and_then(|(start, end)| area.selected(start, end))
            }
            _ => None,
        };
        if let Some((start, end)) = selected
            && let Some(control) = &self.control
        {
            control.selection_changed(id, start, end);
        }
        let change = match self.native(id) {
            Some(Native::Input(field)) => field.compose_if_known(marked),
            Some(Native::Area(area)) => area.compose_if_known(marked),
            _ => None,
        };
        if let Some(change) = change {
            self.report_composition(id, change);
        }
    }

    fn compose(&mut self, id: NodeId, marked: Option<String>) -> Option<(bool, String)> {
        match self.native(id) {
            Some(Native::Input(field)) => field.compose(marked),
            Some(Native::Area(area)) => area.compose(marked),
            _ => None,
        }
    }

    fn report_composition(&self, id: NodeId, (composing, text): (bool, String)) {
        if let Some(control) = &self.control {
            control.composition_changed(id, composing, text);
        }
    }

    fn begin_focus(&mut self, id: NodeId) {
        if self.focused == Some(id) {
            return;
        }
        if let Some(old) = self.focused {
            let ids = self.chain(old);
            self.mark_focused(&ids, false);
        }
        let ids = self.chain(id);
        self.mark_focused(&ids, true);
        self.focused = Some(id);
    }

    fn end_focus(&mut self, id: NodeId) {
        if self.focused != Some(id) {
            return;
        }
        let ids = self.chain(id);
        self.focused = None;
        self.mark_focused(&ids, false);
    }

    fn chain(&self, id: NodeId) -> Vec<NodeId> {
        let Some(view) = self.view(id).map(|view| view.retain()) else {
            return vec![id];
        };
        let mut ids = Vec::new();
        let mut current = Some(view);
        while let Some(view) = current {
            let tag = view.tag() as u32;
            ids.push(tag);
            if tag == ROOT_ID {
                break;
            }
            current = view.superview();
        }
        ids
    }

    fn mark_focused(&mut self, ids: &[NodeId], on: bool) {
        for &id in ids {
            if let Some(slot) = self.slot_mut(id) {
                slot.focused = on;
            }
            self.repaint(id);
        }
    }

    fn native(&self, id: NodeId) -> Option<&Native> {
        self.slot(id).map(|s| &s.native)
    }

    fn view(&self, id: NodeId) -> Option<&UIView> {
        self.native(id).map(|n| n.view())
    }

    fn slot(&self, id: NodeId) -> Option<&Slot> {
        self.slots.get(id as usize)?.as_ref()
    }

    fn slot_mut(&mut self, id: NodeId) -> Option<&mut Slot> {
        self.slots.get_mut(id as usize)?.as_mut()
    }

    fn take(&mut self, id: NodeId) -> Option<Slot> {
        self.slots.get_mut(id as usize)?.take()
    }

    fn ensure(&mut self, id: NodeId) {
        if self.slots.len() <= id as usize {
            self.slots.resize_with(id as usize + 1, || None);
        }
    }

    #[allow(dead_code)] // the host tests read the field; a surface does not.
    pub(crate) fn field(&self, id: NodeId) -> Option<Retained<Field>> {
        match self.native(id)? {
            Native::Input(field) => Some(field.retain()),
            _ => None,
        }
    }

    /// The input's native view, one line or several.
    pub(crate) fn input_view(&self, id: NodeId) -> Option<Retained<UIView>> {
        match self.native(id)? {
            Native::Input(field) => {
                let view: &UIView = &field;
                Some(view.retain())
            }
            Native::Area(area) => {
                let view: &UIView = area.view();
                Some(view.retain())
            }
            _ => None,
        }
    }

    pub(crate) fn scroll(&self, id: NodeId) -> Option<Retained<UIScrollView>> {
        match self.native(id)? {
            Native::Scroll(scroll) => Some(scroll.retain()),
            _ => None,
        }
    }

    pub(crate) fn text(&self, id: NodeId) -> Option<Retained<TextView>> {
        match self.native(id)? {
            Native::Text(view) => Some(view.retain()),
            _ => None,
        }
    }

    pub(crate) fn scene(&self) -> crate::scene::Scene {
        use crate::scene::{ImageSnap, InputSnap, Kind, Node, Rect, ScrollSnap};

        let mut nodes = Vec::new();
        for id in 0..self.slots.len() as u32 {
            let Some(slot) = self.slot(id) else { continue };
            let view = slot.native.view();
            let frame = view.frame();
            let mut node = Node {
                id,
                parent: self.parent_id(view),
                children: Vec::new(),
                kind: match slot.native {
                    Native::View(_) => Kind::View,
                    Native::Text(_) => Kind::Text,
                    Native::Image(_) => Kind::Image,
                    Native::Scroll(_) => Kind::Scroll,
                    Native::Input(_) | Native::Area(_) => Kind::Input,
                },
                test_id: slot.test_id.as_ref().map(|id| id.to_string()),
                accessibility: slot.accessibility.clone(),
                text: None,
                frame: Rect {
                    x: frame.origin.x as f32,
                    y: frame.origin.y as f32,
                    w: frame.size.width as f32,
                    h: frame.size.height as f32,
                },
                hidden: slot.style.hidden,
                opacity: slot.style.opacity,
                pressable: slot.own_press,
                input: None,
                runs: Vec::new(),
                spans: Vec::new(),
                scroll: None,
                image: None,
                transform: Vec::new(),
            };
            if let Native::Text(text) = &slot.native
                && let Some(attributed) = text.attributed()
            {
                node.text = Some(attributed.text.clone());
                node.runs = style_runs(&attributed);
                node.spans = selector_spans(text, &attributed);
            }
            match &slot.native {
                Native::Input(field) => {
                    node.input = Some(InputSnap {
                        value: field.shown(),
                        placeholder: field.placeholder_text(),
                        secure: field.secure(),
                        keyboard: field.keyboard_name().to_owned(),
                        disabled: field.disabled(),
                        focused: field.isFirstResponder(),
                        multiline: false,
                    });
                }
                Native::Area(area) => {
                    node.input = Some(InputSnap {
                        value: area.shown(),
                        placeholder: area.placeholder_text(),
                        secure: area.secure(),
                        keyboard: area.keyboard_name().to_owned(),
                        disabled: area.disabled(),
                        focused: area.view().isFirstResponder(),
                        multiline: true,
                    });
                }
                _ => {}
            }
            if let Native::Scroll(scroll) = &slot.native {
                let offset = scroll.contentOffset();
                let native = scroll.contentSize();
                let (content_w, content_h) = self
                    .relay
                    .ivars()
                    .content
                    .borrow()
                    .get(&id)
                    .copied()
                    .unwrap_or((native.width as f32, native.height as f32));
                node.scroll = Some(ScrollSnap {
                    horizontal: scroll.alwaysBounceHorizontal(),
                    x: offset.x as f32,
                    y: offset.y as f32,
                    content_w,
                    content_h,
                });
            }
            if let Some(picture) = &slot.picture {
                node.image = Some(ImageSnap {
                    src: picture.uri.as_deref().unwrap_or("").to_owned(),
                    fit: fit_name(picture.mode).to_owned(),
                });
            }
            if !slot.style.transform.is_identity() {
                node.transform = slot.style.transform.ops().to_vec();
            }
            let subs = view.subviews();
            for index in 0..subs.count() {
                let child = subs.objectAtIndex(index);
                let child_id = child.tag() as u32;
                if self.slot(child_id).is_some() {
                    node.children.push(child_id);
                }
            }
            nodes.push(node);
        }
        let (width, height) = self
            .view(ROOT_ID)
            .map(|view| {
                let bounds = view.bounds();
                (bounds.size.width as f32, bounds.size.height as f32)
            })
            .unwrap_or((self.viewport.width.0, self.viewport.height.0));
        crate::scene::Scene::from_nodes(nodes, width, height)
    }

    fn parent_id(&self, view: &UIView) -> Option<u32> {
        let parent = view.superview()?;
        let id = parent.tag() as u32;
        let ours = self.view(id)?;
        std::ptr::eq(ours, &*parent).then_some(id)
    }
}

fn style_runs(text: &nlayer_core::text::AttributedText) -> Vec<crate::scene::Run> {
    text.spans
        .iter()
        .map(|span| crate::scene::Run {
            text: crate::scene::utf16_slice(&text.text, span.start, span.end),
            font_size: span.style.font_size.0,
            font_weight: span.style.font_weight,
            italic: span.style.italic,
            color: span.style.color.0,
        })
        .collect()
}

fn selector_spans(
    view: &TextView,
    text: &nlayer_core::text::AttributedText,
) -> Vec<crate::scene::Span> {
    let mut spans = Vec::new();
    for id in &text.test_ids {
        let node = text.links.iter().find_map(|link| {
            (link.start == id.start && link.end == id.end && link.events.contains(EventKind::Press))
                .then_some(link.node)
        });
        spans.push(span_snap(
            view,
            text,
            id.start,
            id.end,
            Some(id.test_id.to_string()),
            node,
        ));
    }
    for link in &text.links {
        if !link.events.contains(EventKind::Press) {
            continue;
        }
        if spans
            .iter()
            .any(|span| span.start == link.start && span.end == link.end)
        {
            continue;
        }
        spans.push(span_snap(
            view,
            text,
            link.start,
            link.end,
            None,
            Some(link.node),
        ));
    }
    spans
}

fn span_snap(
    view: &TextView,
    text: &nlayer_core::text::AttributedText,
    start: u32,
    end: u32,
    test_id: Option<String>,
    node: Option<NodeId>,
) -> crate::scene::Span {
    let (x, y, w, h) = view.span_frame(start, end).unwrap_or((0.0, 0.0, 0.0, 0.0));
    crate::scene::Span {
        start,
        end,
        test_id,
        node,
        text: crate::scene::utf16_slice(&text.text, start, end),
        frame: crate::scene::Rect { x, y, w, h },
    }
}

fn fit_name(mode: ResizeMode) -> &'static str {
    match mode {
        ResizeMode::Cover => "cover",
        ResizeMode::Contain => "contain",
        ResizeMode::Stretch => "fill",
        ResizeMode::Center => "none",
        ResizeMode::ScaleDown => "scale-down",
    }
}

impl Slot {
    fn plain(native: Native) -> Self {
        Self {
            native,
            style: ViewStyle::default(),
            states: [None, None, None],
            pressed: false,
            hovered: false,
            focused: false,
            press: None,
            own_press: false,
            events: EventMask::default(),
            link_press: false,
            hover: None,
            dash: None,
            mask: None,
            sides: [None, None, None, None],
            picture: None,
            test_id: None,
            taps: KeyboardTaps::Never,
            dismiss: KeyboardDismiss::None,
            input_content: None,
            keyboard_tap: None,
            accessibility: nlayer_core::Accessibility::default(),
            bindings: Vec::new(),
        }
    }
}

/// A tap on a scroll view that resigns the keyboard. Its delegate lets
/// through only taps that should.
fn keyboard_tap(
    mtm: MainThreadMarker,
    relay: &Relay,
    view: &UIView,
) -> Retained<UITapGestureRecognizer> {
    // SAFETY: the relay implements `dismissKeyboard:` and outlives the scroll.
    let tap = unsafe {
        UITapGestureRecognizer::initWithTarget_action(
            UITapGestureRecognizer::alloc(mtm),
            Some(relay),
            Some(sel!(dismissKeyboard:)),
        )
    };
    tap.setCancelsTouchesInView(false);
    tap.setDelegate(Some(ProtocolObject::from_ref(relay)));
    view.addGestureRecognizer(&tap);
    tap
}

fn state_index(interaction: Interaction) -> usize {
    match interaction {
        Interaction::Active => 0,
        Interaction::Hover => 1,
        Interaction::Focus => 2,
    }
}

/// The view paint on screen: the base style, with the winning state's
/// non-default fields over it. Active beats hover, which beats focus.
fn painted(slot: &Slot) -> ViewStyle {
    let mut style = slot.style.clone();
    let over = winning(slot);
    let Some(over) = over else { return style };
    let over = &over.view;
    if !over.background_color.is_transparent() {
        style.background_color = over.background_color;
    }
    if (over.opacity - 1.0).abs() > f32::EPSILON {
        style.opacity = over.opacity;
    }
    if over.border_radius != Points::ZERO || over.radii() != ViewStyle::default().radii() {
        style.border_radius = over.border_radius;
        style.corner_radius = over.corner_radius;
        style.continuous_corners = over.continuous_corners;
    }
    if over.borders.is_visible() {
        style.borders = over.borders;
    }
    if !over.transform.is_identity() {
        style.transform = over.transform;
        style.transform_origin = over.transform_origin;
    }
    if over.overflow_hidden {
        style.overflow_hidden = true;
    }
    if over.hidden {
        style.hidden = true;
    }
    style
}

fn text_override(slot: &Slot) -> Option<Color> {
    winning(slot)?.text.color
}

fn winning(slot: &Slot) -> Option<&Paint> {
    if slot.pressed {
        slot.states[state_index(Interaction::Active)].as_ref()
    } else if slot.hovered {
        slot.states[state_index(Interaction::Hover)].as_ref()
    } else if slot.focused {
        slot.states[state_index(Interaction::Focus)].as_ref()
    } else {
        None
    }
}

fn paint_view(view: &UIView, style: &ViewStyle, slot: &mut Slot, scroll: bool) {
    let layer = view.layer();
    view.setAlpha(style.opacity as CGFloat);
    view.setHidden(style.hidden);
    let color = if style.background_color.is_transparent() {
        None
    } else {
        Some(crate::text::cg_color(style.background_color))
    };
    layer.setBackgroundColor(color.as_deref());
    let bounds = view.bounds();
    let size = Size::new(
        Points(bounds.size.width as f32),
        Points(bounds.size.height as f32),
    );
    let radii = style.radii().fit(size);
    apply_corners(&layer, radii, bounds, &mut slot.mask);
    // A drag moves children with the content offset. The scroll viewport
    // clips them, the way `overflow: auto` does. `overflow` only clips a
    // plain view.
    layer.setMasksToBounds(style.overflow_hidden || scroll);
    apply_border(view, style, slot);
    layer.setTransform(transform_3d(style, bounds.size));
}

fn apply_corners(
    layer: &CALayer,
    radii: nlayer_core::style::CornerRadii,
    bounds: CGRect,
    mask: &mut Option<Retained<CAShapeLayer>>,
) {
    if let Some(shared) = radii.shared() {
        if mask.take().is_some() {
            // SAFETY: clearing a mask this slot installed.
            unsafe { layer.setMask(None) };
        }
        layer.setCornerRadius(shared.0 as CGFloat);
        // MinXMinY is the top left in UIKit's y-down space.
        let mut bits: usize = 0;
        if radii.top_left > Points::ZERO {
            bits |= 1 << 0;
        }
        if radii.top_right > Points::ZERO {
            bits |= 1 << 1;
        }
        if radii.bottom_left > Points::ZERO {
            bits |= 1 << 2;
        }
        if radii.bottom_right > Points::ZERO {
            bits |= 1 << 3;
        }
        layer.setMaskedCorners(objc2_quartz_core::CACornerMask(bits));
        return;
    }
    layer.setCornerRadius(0.0);
    let shape = mask.get_or_insert_with(CAShapeLayer::layer);
    shape.setFrame(bounds);
    shape.setPath(Some(&rounded_path(bounds, radii)));
    // SAFETY: a mask layer owned by this slot; the layer retains it.
    unsafe { layer.setMask(Some(shape)) };
}

fn rounded_path(
    rect: CGRect,
    radii: nlayer_core::style::CornerRadii,
) -> Retained<objc2_core_graphics::CGPath> {
    use objc2_ui_kit::UIBezierPath;
    let path = UIBezierPath::bezierPath();
    let (x, y, w, h) = (
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    );
    let (tl, tr, br, bl) = (
        radii.top_left.0 as CGFloat,
        radii.top_right.0 as CGFloat,
        radii.bottom_right.0 as CGFloat,
        radii.bottom_left.0 as CGFloat,
    );
    path.moveToPoint(CGPoint::new(x + tl, y));
    path.addLineToPoint(CGPoint::new(x + w - tr, y));
    if tr > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + w - tr, y + tr),
            tr,
            -std::f64::consts::FRAC_PI_2,
            0.0,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x + w, y + h - br));
    if br > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + w - br, y + h - br),
            br,
            0.0,
            std::f64::consts::FRAC_PI_2,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x + bl, y + h));
    if bl > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + bl, y + h - bl),
            bl,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x, y + tl));
    if tl > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + tl, y + tl),
            tl,
            std::f64::consts::PI,
            std::f64::consts::PI + std::f64::consts::FRAC_PI_2,
            true,
        );
    }
    path.closePath();
    path.CGPath().clone()
}

fn apply_border(view: &UIView, style: &ViewStyle, slot: &mut Slot) {
    let layer = view.layer();
    let borders = &style.borders;
    if let Some((width, color)) = borders.uniform() {
        clear_sides(slot);
        if borders.style == BorderStyle::Solid || width <= Points::ZERO {
            if let Some(dash) = slot.dash.take() {
                dash.removeFromSuperlayer();
            }
            layer.setBorderWidth(width.0 as CGFloat);
            layer.setBorderColor(Some(&*crate::text::cg_color(color)));
            return;
        }
        layer.setBorderWidth(0.0);
        let dash = slot.dash.get_or_insert_with(CAShapeLayer::layer);
        if dash.superlayer().is_none() {
            layer.addSublayer(dash);
        }
        let inset = width.0 as CGFloat / 2.0;
        let bounds = view.bounds();
        let rect = CGRect::new(
            CGPoint::new(bounds.origin.x + inset, bounds.origin.y + inset),
            CGSize::new(
                (bounds.size.width - width.0 as CGFloat).max(0.0),
                (bounds.size.height - width.0 as CGFloat).max(0.0),
            ),
        );
        let radii = style.radii().fit(Size::new(
            Points(bounds.size.width as f32),
            Points(bounds.size.height as f32),
        ));
        let path = rounded_path(rect, radii.inset(Points(inset as f32)));
        dash.setFrame(bounds);
        dash.setPath(Some(&path));
        dash.setFillColor(None);
        dash.setStrokeColor(Some(&*crate::text::cg_color(color)));
        dash.setLineWidth(width.0 as CGFloat);
        dash.setLineDashPattern(Some(&dash_pattern(borders.style == BorderStyle::Dotted)));
        dash.setZPosition(1_000.0);
        return;
    }
    layer.setBorderWidth(0.0);
    if let Some(dash) = slot.dash.take() {
        dash.removeFromSuperlayer();
    }
    let bounds = view.bounds();
    let (w, h) = (bounds.size.width, bounds.size.height);
    let frames = [
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(w, borders.widths.top.0 as CGFloat),
        ),
        CGRect::new(
            CGPoint::new(w - borders.widths.right.0 as CGFloat, 0.0),
            CGSize::new(borders.widths.right.0 as CGFloat, h),
        ),
        CGRect::new(
            CGPoint::new(0.0, h - borders.widths.bottom.0 as CGFloat),
            CGSize::new(w, borders.widths.bottom.0 as CGFloat),
        ),
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(borders.widths.left.0 as CGFloat, h),
        ),
    ];
    for (i, frame) in frames.into_iter().enumerate() {
        let visible = borders.width(i) > Points::ZERO && !borders.colors[i].is_transparent();
        if !visible {
            if let Some(side) = slot.sides[i].take() {
                side.removeFromSuperlayer();
            }
            continue;
        }
        let side = slot.sides[i].get_or_insert_with(CALayer::layer);
        if side.superlayer().is_none() {
            layer.addSublayer(side);
        }
        side.setFrame(frame);
        side.setBackgroundColor(Some(&*crate::text::cg_color(borders.colors[i])));
        side.setZPosition(1_000.0);
    }
}

fn clear_sides(slot: &mut Slot) {
    for side in &mut slot.sides {
        if let Some(side) = side.take() {
            side.removeFromSuperlayer();
        }
    }
}

/// Keeps the bounds origin, which is a scroll view's content offset.
/// Returns whether the size changed. A move with the same size does not
/// need a repaint.
fn set_frame(view: &UIView, frame: Bounds<Points>) -> bool {
    let bounds = view.bounds();
    let width = frame.size.width.0 as CGFloat;
    let height = frame.size.height.0 as CGFloat;
    let resized = bounds.size.width != width || bounds.size.height != height;
    let center = CGPoint::new(
        frame.origin.x.0 as CGFloat + width / 2.0,
        frame.origin.y.0 as CGFloat + height / 2.0,
    );
    let placed = view.center();
    if resized || placed.x != center.x || placed.y != center.y {
        view.setBounds(CGRect::new(bounds.origin, CGSize::new(width, height)));
        view.setCenter(center);
    }
    resized
}

fn transform_3d(style: &ViewStyle, size: CGSize) -> CATransform3D {
    let mut m = style.transform.matrix(|_| None).0;
    let origin = style.transform_origin;
    if origin != TransformOrigin::CENTER {
        let (d, dz) = origin.offset(Size::new(
            Points(size.width as f32),
            Points(size.height as f32),
        ));
        let (dx, dy, dz) = (d.x.0 as CGFloat, d.y.0 as CGFloat, dz.0 as CGFloat);
        m = mat_mul(
            &mat_mul(&translation(-dx, -dy, -dz), &m.map(|n| n as CGFloat)),
            &translation(dx, dy, dz),
        );
    }
    CATransform3D {
        m11: m[0] as CGFloat,
        m12: m[1] as CGFloat,
        m13: m[2] as CGFloat,
        m14: m[3] as CGFloat,
        m21: m[4] as CGFloat,
        m22: m[5] as CGFloat,
        m23: m[6] as CGFloat,
        m24: m[7] as CGFloat,
        m31: m[8] as CGFloat,
        m32: m[9] as CGFloat,
        m33: m[10] as CGFloat,
        m34: m[11] as CGFloat,
        m41: m[12] as CGFloat,
        m42: m[13] as CGFloat,
        m43: m[14] as CGFloat,
        m44: m[15] as CGFloat,
    }
}

/// A translation matrix (row vectors, as Core Animation).
fn translation(x: CGFloat, y: CGFloat, z: CGFloat) -> [CGFloat; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        x, y, z, 1.0,
    ]
}

fn mat_mul(a: &[CGFloat; 16], b: &[CGFloat; 16]) -> [CGFloat; 16] {
    let mut out = [0.0; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = (0..4).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
    }
    out
}

/// The bitmap layer of an image view, clipped to the view's box.
struct Picture {
    clip: Retained<CALayer>,
    layer: Retained<CALayer>,
    mode: ResizeMode,
    uri: Option<Arc<str>>,
    generation: u64,
    /// Natural size in points, once decoded (`None` until then).
    natural: Option<Size<Points>>,
    /// Clip shape when the four radii differ. Reused across paints.
    mask: Option<Retained<CAShapeLayer>>,
}

impl Mounter {
    fn set_image(&mut self, id: NodeId, source: Option<ImageSource>, mode: ResizeMode) {
        let scale = screen_scale(self.mtm);
        let reload = {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            let Some(picture) = &mut slot.picture else {
                return;
            };
            picture.mode = mode;
            let next = source.and_then(|source| (!source.uri.is_empty()).then_some(source.uri));
            if picture.uri == next {
                None
            } else {
                picture.uri = next.clone();
                picture.generation = picture.generation.wrapping_add(1);
                picture.natural = None;
                let generation = picture.generation;
                let layer = picture.layer.clone();
                quiet(|| unsafe { layer.setContents(None) });
                Some((generation, next))
            }
        };
        if let Some((generation, Some(uri))) = reload
            && let Some(decoded) = begin_load(id, generation, uri, &self.assets, scale)
        {
            self.finish_image(id, generation, scale, Ok(decoded));
            return;
        }
        self.repaint(id);
    }

    fn finish_image(
        &mut self,
        id: NodeId,
        generation: u64,
        scale: f32,
        loaded: Result<crate::image::Decoded, String>,
    ) {
        let decoded = match loaded {
            Ok(decoded) => Some(decoded),
            Err(error) => {
                eprintln!("nlayer: image: {error}");
                None
            }
        };
        {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            let Some(picture) = &mut slot.picture else {
                return;
            };
            if picture.generation != generation {
                return;
            }
            let Some(decoded) = decoded else {
                return;
            };
            picture.natural = Some(Size::new(
                Points(decoded.width as f32 / scale),
                Points(decoded.height as f32 / scale),
            ));
            let layer = picture.layer.clone();
            quiet(|| {
                layer.setContentsScale(scale as CGFloat);
                // SAFETY: a CGImage is a valid layer `contents` object, and
                // the layer retains it.
                unsafe {
                    layer.setContents(Some(
                        &*(&*decoded.image as *const CGImage as *const AnyObject),
                    ));
                }
            });
        }
        self.repaint(id);
    }
}

struct Waiter {
    id: NodeId,
    generation: u64,
    scale: f32,
}

fn inflight() -> &'static Mutex<HashMap<Arc<str>, Vec<Waiter>>> {
    static INFLIGHT: OnceLock<Mutex<HashMap<Arc<str>, Vec<Waiter>>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Starts a decode of `uri`, or joins one already running. A cached bitmap
/// is returned so the caller can paint it now.
fn begin_load(
    id: NodeId,
    generation: u64,
    uri: Arc<str>,
    assets: &std::path::Path,
    scale: f32,
) -> Option<crate::image::Decoded> {
    if let Some(decoded) = crate::image::cached(&uri) {
        return Some(decoded);
    }
    {
        let mut inflight = inflight().lock().expect("image loads");
        if let Some(waiters) = inflight.get_mut(&uri) {
            waiters.push(Waiter {
                id,
                generation,
                scale,
            });
            return None;
        }
        inflight.insert(
            Arc::clone(&uri),
            vec![Waiter {
                id,
                generation,
                scale,
            }],
        );
    }
    let assets = assets.to_path_buf();
    let key = uri;
    use dispatch2::{DispatchQueueGlobalPriority, GlobalQueueIdentifier};
    DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
        DispatchQueueGlobalPriority::Default,
    ))
    .exec_async(move || {
        let loaded = crate::image::load(&assets, &key).map_err(|error| format!("{key}: {error}"));
        DispatchQueue::main().exec_async(move || {
            if let Ok(decoded) = &loaded {
                crate::image::store(Arc::clone(&key), decoded);
            }
            let waiters = inflight()
                .lock()
                .expect("image loads")
                .remove(&key)
                .unwrap_or_default();
            let Some(mounter) = current() else {
                return;
            };
            let mut mounter = mounter.borrow_mut();
            for waiter in waiters {
                mounter.finish_image(waiter.id, waiter.generation, waiter.scale, loaded.clone());
            }
        });
    });
    None
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Rc<std::cell::RefCell<Mounter>>>> =
        const { std::cell::RefCell::new(None) };
}

fn current() -> Option<Rc<std::cell::RefCell<Mounter>>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

/// Runs `f` on the mounter. `endEditing:` posts the keyboard notification
/// and `editingEnded:` while a dismiss or a batch may already hold it.
/// Then the call waits for the next main-queue turn, as nsh's
/// `with_mounter` does, instead of re-entering the `RefCell`.
pub(crate) fn with_mounter(f: impl FnOnce(&mut Mounter) + Send + 'static) {
    if let Some(shared) = current()
        && let Ok(mut mounter) = shared.try_borrow_mut()
    {
        f(&mut mounter);
        return;
    }
    DispatchQueue::main().exec_async(move || {
        let Some(shared) = current() else {
            return;
        };
        if let Ok(mut mounter) = shared.try_borrow_mut() {
            f(&mut mounter);
        }
    });
}

/// Resigns the focused field without holding the mounter. `endEditing:`
/// posts the keyboard notification and `editingEnded:` before it returns.
fn resign_keyboard(shared: &Rc<std::cell::RefCell<Mounter>>) {
    let window = {
        let mounter = shared.borrow();
        mounter.view(ROOT_ID).and_then(|view| view.window())
    };
    if let Some(window) = window {
        // SAFETY: `endEditing:` walks the window's responders.
        let _: bool = unsafe { msg_send![&*window, endEditing: true] };
        return;
    }
    let field = {
        let mounter = shared.borrow();
        mounter.focused.and_then(|id| mounter.input_view(id))
    };
    if let Some(field) = field {
        let _: bool = unsafe { msg_send![&*field, resignFirstResponder] };
    }
}

fn image_layers(mtm: MainThreadMarker) -> (Retained<UIView>, Picture) {
    let view = UIView::new(mtm);
    let clip = CALayer::layer();
    clip.setMasksToBounds(true);
    let layer = CALayer::layer();
    // SAFETY: an immutable Core Animation constant.
    unsafe { layer.setContentsGravity(kCAGravityResize) };
    clip.addSublayer(&layer);
    view.layer().insertSublayer_atIndex(&clip, 0);
    (
        view,
        Picture {
            clip,
            layer,
            mode: ResizeMode::Cover,
            uri: None,
            generation: 0,
            natural: None,
            mask: None,
        },
    )
}

/// Frames the picture where `objectFit` puts it, and clips that to the view.
fn place_image(view: &UIView, style: &ViewStyle, slot: &mut Slot) {
    let Some(picture) = &mut slot.picture else {
        return;
    };
    let bounds = view.bounds();
    let size = Size::new(
        Points(bounds.size.width as f32),
        Points(bounds.size.height as f32),
    );
    let radii = style.radii().fit(size);
    let fitted = match picture.natural {
        Some(natural) => picture.mode.fit(natural, size, style.object_position),
        None => Bounds {
            origin: Point::default(),
            size: Size::default(),
        },
    };
    let clip = picture.clip.clone();
    let layer = picture.layer.clone();
    clip.setFrame(bounds);
    apply_corners(&clip, radii, clip.bounds(), &mut picture.mask);
    layer.setFrame(CGRect::new(
        CGPoint::new(fitted.origin.x.0 as CGFloat, fitted.origin.y.0 as CGFloat),
        CGSize::new(
            fitted.size.width.0 as CGFloat,
            fitted.size.height.0 as CGFloat,
        ),
    ));
}

fn dash_pattern(dotted: bool) -> Retained<objc2_foundation::NSArray<objc2_foundation::NSNumber>> {
    use objc2_foundation::{NSArray, NSNumber};
    use std::cell::RefCell;
    thread_local! {
        static DASHED: RefCell<Option<Retained<NSArray<NSNumber>>>> = const { RefCell::new(None) };
        static DOTTED: RefCell<Option<Retained<NSArray<NSNumber>>>> = const { RefCell::new(None) };
    }
    let fill = |slot: &RefCell<Option<Retained<NSArray<NSNumber>>>>, pattern: &[f64]| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let dashes: Vec<_> = pattern
                .iter()
                .copied()
                .map(NSNumber::numberWithDouble)
                .collect();
            *slot = Some(NSArray::from_retained_slice(&dashes));
        }
        slot.as_ref().expect("dash pattern").clone()
    };
    if dotted {
        DOTTED.with(|slot| fill(slot, &[1.0, 2.0]))
    } else {
        DASHED.with(|slot| fill(slot, &[4.0, 3.0]))
    }
}

thread_local! {
    static QUIET: Cell<u32> = const { Cell::new(0) };
}

struct QuietGuard {
    outer: bool,
}

impl Drop for QuietGuard {
    fn drop(&mut self) {
        if self.outer {
            CATransaction::commit();
        }
        QUIET.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Runs `f` without Core Animation's implicit animations. Nested calls
/// share the outer transaction, so a batch is one commit.
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let outer = QUIET.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current == 0
    });
    if outer {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
    }
    let _guard = QuietGuard { outer };
    f()
}

#[allow(deprecated)]
fn screen_scale(mtm: MainThreadMarker) -> f32 {
    let scale = UIScreen::mainScreen(mtm).scale() as f32;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

#[cfg(feature = "host-tests")]
pub(crate) mod tests {
    use super::*;
    use nlayer_core::geometry::{Bounds, point, px, size};
    use nlayer_core::text::{AttributedText, ResolvedTextStyle, TextEngine, TextSpan};

    /// The tests own the main thread (the runner is a `harness = false`
    /// binary): UIKit views, CoreText and the run loop's queue are
    /// main-thread only.
    fn mounter() -> Rc<RefCell<Mounter>> {
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let root = UIView::new(mtm);
        root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
        Mounter::new(mtm, root, PathBuf::new())
    }

    fn frame(x: f32, y: f32, width: f32, height: f32) -> Bounds<Points> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn child(id: NodeId, kind: ViewKind, at: Bounds<Points>) -> Vec<MountItem> {
        vec![
            MountItem::Create { id, kind },
            MountItem::Insert {
                parent: ROOT_ID,
                child: id,
                before: None,
            },
            MountItem::Frame { id, frame: at },
        ]
    }

    fn apply(mounter: &Rc<RefCell<Mounter>>, items: Vec<MountItem>) {
        mounter.borrow_mut().apply(MountBatch { items });
    }

    pub(crate) fn tests() -> Vec<(&'static str, fn())> {
        vec![
            (
                "mount::a_batch_frames_paints_and_names_a_view",
                a_batch_frames_paints_and_names_a_view as fn(),
            ),
            (
                "mount::a_transform_origin_follows_the_bounds",
                a_transform_origin_follows_the_bounds as fn(),
            ),
            (
                "mount::active_beats_hover_and_both_beat_the_base_paint",
                active_beats_hover_and_both_beat_the_base_paint as fn(),
            ),
            (
                "mount::a_focused_field_lights_the_focus_style_on_its_ancestors",
                a_focused_field_lights_the_focus_style_on_its_ancestors as fn(),
            ),
            (
                "mount::a_vertical_scroll_narrows_its_native_width_until_a_row_grows",
                a_vertical_scroll_narrows_its_native_width_until_a_row_grows as fn(),
            ),
            (
                "mount::scroll_by_and_scroll_to_move_a_horizontal_scroll",
                scroll_by_and_scroll_to_move_a_horizontal_scroll as fn(),
            ),
            (
                "mount::an_input_takes_its_configuration_and_value",
                an_input_takes_its_configuration_and_value as fn(),
            ),
            (
                "mount::a_return_resigns_text_and_email_but_not_number",
                a_return_resigns_text_and_email_but_not_number as fn(),
            ),
            (
                "mount::an_image_records_its_source_and_fit",
                an_image_records_its_source_and_fit as fn(),
            ),
            (
                "mount::an_image_decodes_and_clips_to_its_radius",
                an_image_decodes_and_clips_to_its_radius as fn(),
            ),
            (
                "mount::a_text_block_carries_its_layout_and_runs",
                a_text_block_carries_its_layout_and_runs as fn(),
            ),
        ]
    }

    fn a_batch_frames_paints_and_names_a_view() {
        let mounter = mounter();
        let mut style = ViewStyle::default();
        style.opacity = 0.5;
        style.hidden = true;
        style.border_radius = px(8.0);
        style.overflow_hidden = true;
        style.background_color = Color::rgba(1, 2, 3, 255);
        apply(
            &mounter,
            [
                child(2, ViewKind::View, frame(10.0, 20.0, 100.0, 50.0)),
                vec![
                    MountItem::ViewStyle {
                        id: 2,
                        style: Box::new(style),
                    },
                    MountItem::TestId {
                        id: 2,
                        test_id: Some("card".into()),
                    },
                ],
            ]
            .concat(),
        );

        let view = mounter.borrow().view_of(2).expect("view");
        assert_eq!(view.frame().origin.x, 10.0);
        assert_eq!(view.frame().origin.y, 20.0);
        assert_eq!(view.frame().size.width, 100.0);
        assert_eq!(view.frame().size.height, 50.0);
        assert!((view.alpha() - 0.5).abs() < 1e-6);
        assert!(view.isHidden());
        assert!(view.layer().masksToBounds());
        assert_eq!(view.layer().cornerRadius(), 8.0);

        let tree = mounter.borrow().scene().tree();
        let node = &tree["children"][0];
        assert_eq!(node["kind"], "view");
        assert_eq!(node["testID"], "card");
        assert_eq!(node["frame"]["x"], 10.0);
        assert_eq!(node["frame"]["height"], 50.0);

        // A delete drops the slot and the native view.
        apply(&mounter, vec![MountItem::Delete { id: 2 }]);
        assert!(mounter.borrow().view_of(2).is_none());
        assert_eq!(
            mounter.borrow().scene().tree()["children"]
                .as_array()
                .map(Vec::len),
            Some(0)
        );
    }

    fn a_transform_origin_follows_the_bounds() {
        let mounter = mounter();
        let mut parsed = nlayer_core::style::Style::default();
        parsed
            .set(
                "transform",
                nlayer_core::style::StyleValue::Str("rotate(90deg)"),
            )
            .unwrap();
        parsed
            .set(
                "transformOrigin",
                nlayer_core::style::StyleValue::Str("25% 75%"),
            )
            .unwrap();
        apply(
            &mounter,
            [
                child(2, ViewKind::View, frame(0.0, 0.0, 20.0, 10.0)),
                vec![MountItem::ViewStyle {
                    id: 2,
                    style: Box::new(parsed.view),
                }],
            ]
            .concat(),
        );
        let view = mounter.borrow().view_of(2).expect("view");
        let turned = view.layer().transform();
        // The origin is (5, 7.5), 5 left of and 2.5 below the center.
        assert!((turned.m41 + 2.5).abs() < 1e-6, "{}", turned.m41);
        assert!((turned.m42 - 7.5).abs() < 1e-6, "{}", turned.m42);

        // A percentage origin is of the new size, not the first one.
        apply(
            &mounter,
            vec![MountItem::Frame {
                id: 2,
                frame: frame(0.0, 0.0, 40.0, 20.0),
            }],
        );
        let turned = view.layer().transform();
        assert!((turned.m41 + 5.0).abs() < 1e-6, "{}", turned.m41);
        assert!((turned.m42 - 15.0).abs() < 1e-6, "{}", turned.m42);
    }

    fn active_beats_hover_and_both_beat_the_base_paint() {
        let mounter = mounter();
        let mut hover = ViewStyle::default();
        hover.opacity = 0.4;
        let mut active = ViewStyle::default();
        active.opacity = 0.2;
        apply(
            &mounter,
            [
                child(2, ViewKind::View, frame(0.0, 0.0, 100.0, 50.0)),
                vec![
                    MountItem::StateStyle {
                        id: 2,
                        interaction: Interaction::Hover,
                        view: Box::new(hover),
                        text: TextStyle::default(),
                    },
                    MountItem::StateStyle {
                        id: 2,
                        interaction: Interaction::Active,
                        view: Box::new(active),
                        text: TextStyle::default(),
                    },
                ],
            ]
            .concat(),
        );
        let view = mounter.borrow().view_of(2).expect("view");
        assert!((view.alpha() - 1.0).abs() < 1e-6);

        mounter
            .borrow_mut()
            .hovered(2, UIGestureRecognizerState::Began);
        assert!((view.alpha() - 0.4).abs() < 1e-6);

        mounter
            .borrow_mut()
            .pressed(2, UIGestureRecognizerState::Began, CGPoint::ZERO);
        assert!((view.alpha() - 0.2).abs() < 1e-6);

        mounter
            .borrow_mut()
            .pressed(2, UIGestureRecognizerState::Ended, CGPoint::ZERO);
        assert!((view.alpha() - 0.4).abs() < 1e-6);

        mounter
            .borrow_mut()
            .hovered(2, UIGestureRecognizerState::Ended);
        assert!((view.alpha() - 1.0).abs() < 1e-6);
    }

    fn a_focused_field_lights_the_focus_style_on_its_ancestors() {
        let mounter = mounter();
        let mut focus = ViewStyle::default();
        focus.opacity = 0.25;
        apply(
            &mounter,
            [
                child(2, ViewKind::View, frame(0.0, 0.0, 200.0, 100.0)),
                vec![
                    MountItem::Create {
                        id: 3,
                        kind: ViewKind::Input,
                    },
                    MountItem::Insert {
                        parent: 2,
                        child: 3,
                        before: None,
                    },
                    MountItem::Frame {
                        id: 3,
                        frame: frame(0.0, 0.0, 200.0, 40.0),
                    },
                    MountItem::StateStyle {
                        id: 2,
                        interaction: Interaction::Focus,
                        view: Box::new(focus),
                        text: TextStyle::default(),
                    },
                ],
            ]
            .concat(),
        );
        let parent = mounter.borrow().view_of(2).expect("parent");
        assert!((parent.alpha() - 1.0).abs() < 1e-6);

        mounter.borrow_mut().begin_focus(3);
        assert!((parent.alpha() - 0.25).abs() < 1e-6);

        mounter.borrow_mut().end_focus(3);
        assert!((parent.alpha() - 1.0).abs() < 1e-6);
    }

    fn a_vertical_scroll_narrows_its_native_width_until_a_row_grows() {
        let mounter = mounter();
        apply(
            &mounter,
            [
                child(2, ViewKind::Scroll, frame(0.0, 0.0, 320.0, 200.0)),
                vec![
                    MountItem::ScrollConfig {
                        id: 2,
                        horizontal: false,
                        taps: KeyboardTaps::Never,
                        dismiss: KeyboardDismiss::None,
                    },
                    MountItem::Create {
                        id: 3,
                        kind: ViewKind::View,
                    },
                    MountItem::Insert {
                        parent: 2,
                        child: 3,
                        before: None,
                    },
                    MountItem::Frame {
                        id: 3,
                        frame: frame(0.0, 0.0, 300.0, 50.0),
                    },
                    MountItem::ScrollContent {
                        id: 2,
                        size: size(px(500.0), px(1000.0)),
                    },
                ],
            ]
            .concat(),
        );
        let scene = mounter.borrow().scene();
        let tree = scene.tree();
        assert_eq!(tree["children"][0]["content"]["width"], 500.0);

        let scroll = mounter.borrow().scroll(2).expect("scroll view");
        // Nothing inside passes the viewport's right edge, so the page
        // would pan over nothing: the native width narrows to the viewport.
        assert_eq!(scroll.contentSize().width, 320.0);
        // The event and the tree still report the engine's size.
        drop(scene);

        apply(
            &mounter,
            vec![MountItem::Frame {
                id: 3,
                frame: frame(0.0, 0.0, 400.0, 50.0),
            }],
        );
        // The row grew past the viewport. The engine sends no new content
        // size for that batch, so the narrowed width is given back here.
        assert_eq!(scroll.contentSize().width, 500.0);
    }

    fn scroll_by_and_scroll_to_move_a_horizontal_scroll() {
        let mounter = mounter();
        apply(
            &mounter,
            [
                child(2, ViewKind::Scroll, frame(0.0, 0.0, 300.0, 100.0)),
                vec![
                    MountItem::ScrollConfig {
                        id: 2,
                        horizontal: true,
                        taps: KeyboardTaps::Never,
                        dismiss: KeyboardDismiss::None,
                    },
                    MountItem::ScrollContent {
                        id: 2,
                        size: size(px(600.0), px(100.0)),
                    },
                    MountItem::ScrollBy {
                        id: 2,
                        dx: px(40.0),
                        dy: px(0.0),
                    },
                ],
            ]
            .concat(),
        );
        let scroll = mounter.borrow().scroll(2).expect("scroll view");
        assert_eq!(scroll.contentOffset().x, 40.0);
        assert!(scroll.alwaysBounceHorizontal());

        apply(
            &mounter,
            vec![MountItem::Command {
                id: 2,
                command: nlayer_core::Command::ScrollTo {
                    x: px(10.0),
                    y: px(0.0),
                    animated: false,
                },
            }],
        );
        assert_eq!(scroll.contentOffset().x, 10.0);
    }

    fn an_input_takes_its_configuration_and_value() {
        let mounter = mounter();
        let config = nlayer_core::InputConfig {
            placeholder: "Name".into(),
            secure: true,
            keyboard: nlayer_core::KeyboardType::Email,
            disabled: true,
            ..nlayer_core::InputConfig::default()
        };
        apply(
            &mounter,
            [
                child(2, ViewKind::Input, frame(0.0, 0.0, 200.0, 40.0)),
                vec![
                    MountItem::Input {
                        id: 2,
                        config: Box::new(config),
                    },
                    MountItem::InputValue {
                        id: 2,
                        text: "ada".into(),
                        count: 0,
                    },
                    MountItem::TestId {
                        id: 2,
                        test_id: Some("name".into()),
                    },
                ],
            ]
            .concat(),
        );

        let field = mounter.borrow().field(2).expect("field");
        assert_eq!(field.shown(), "ada");
        assert_eq!(field.placeholder_text(), "Name");
        assert!(field.secure());
        assert_eq!(field.keyboard_name(), "email");
        assert!(field.disabled());

        let tree = mounter.borrow().scene().tree();
        let node = &tree["children"][0];
        assert_eq!(node["kind"], "input");
        assert_eq!(node["value"], "ada");
        assert_eq!(node["placeholder"], "Name");
        assert_eq!(node["secure"], true);
        assert_eq!(node["keyboard"], "email");
        assert_eq!(node["disabled"], true);
    }

    fn a_return_resigns_text_and_email_but_not_number() {
        use objc2_ui_kit::UIWindow;

        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let bounds = CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 480.0));
        let window = UIWindow::initWithFrame(UIWindow::alloc(mtm), bounds);
        let root = UIView::new(mtm);
        root.setFrame(bounds);
        window.addSubview(&root);
        window.makeKeyAndVisible();
        let mounter = Mounter::new(mtm, root, PathBuf::new());

        let keyboards = [
            (2, nlayer_core::KeyboardType::Text, true),
            (3, nlayer_core::KeyboardType::Email, true),
            (4, nlayer_core::KeyboardType::Number, false),
        ];
        let mut items = Vec::new();
        for (id, keyboard, _) in keyboards {
            items.extend(child(id, ViewKind::Input, frame(0.0, 0.0, 200.0, 40.0)));
            items.push(MountItem::Input {
                id,
                config: Box::new(nlayer_core::InputConfig {
                    keyboard,
                    ..nlayer_core::InputConfig::default()
                }),
            });
        }
        apply(&mounter, items);

        for (id, keyboard, resigns) in keyboards {
            let field = mounter.borrow().field(id).expect("field").retain();
            assert!(
                field.becomeFirstResponder(),
                "{keyboard:?} takes the keyboard"
            );
            // `EditingDidEndOnExit` is what UIKit sends when Return is pressed;
            // the handler resigns text and email only.
            let relay = mounter.borrow().relay.clone();
            // SAFETY: `Relay` implements `returned:`.
            let _: () = unsafe { objc2::msg_send![&*relay, returned: &*field] };
            assert_eq!(
                field.isFirstResponder(),
                !resigns,
                "{keyboard:?}: Return resigns text and email, not number"
            );
        }
        drop(window);
    }

    fn an_image_records_its_source_and_fit() {
        let mounter = mounter();
        apply(
            &mounter,
            [
                child(2, ViewKind::Image, frame(0.0, 0.0, 32.0, 32.0)),
                vec![MountItem::Image {
                    id: 2,
                    source: Some(ImageSource {
                        uri: "asset:/missing.png".into(),
                        size: None,
                        scales: nlayer_core::ImageScales::NONE,
                    }),
                    resize_mode: ResizeMode::Contain,
                }],
            ]
            .concat(),
        );
        let tree = mounter.borrow().scene().tree();
        assert_eq!(tree["children"][0]["kind"], "image");
        assert_eq!(tree["children"][0]["src"], "asset:/missing.png");
        assert_eq!(tree["children"][0]["fit"], "contain");
    }

    fn an_image_decodes_and_clips_to_its_radius() {
        // A 4×2 red PNG: the smallest file ImageIO still decodes.
        const PNG: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2,
            8, 6, 0, 0, 0, 127, 168, 125, 99, 0, 0, 0, 18, 73, 68, 65, 84, 120, 218, 99, 248, 207,
            192, 240, 31, 25, 51, 160, 11, 0, 0, 15, 33, 15, 241, 254, 69, 20, 99, 0, 0, 0, 0, 73,
            69, 78, 68, 174, 66, 96, 130,
        ];
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let root = UIView::new(mtm);
        root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
        let assets = std::env::temp_dir().join(format!(
            "nlayer-image-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&assets).expect("temporary assets");
        std::fs::write(assets.join("pic.png"), PNG).expect("a PNG on disk");
        let mounter = Mounter::new(mtm, root, assets.clone());

        let mut parsed = nlayer_core::style::Style::default();
        parsed
            .set("borderRadius", nlayer_core::style::StyleValue::Number(8.0))
            .unwrap();
        apply(
            &mounter,
            [
                child(2, ViewKind::Image, frame(0.0, 0.0, 32.0, 32.0)),
                vec![
                    MountItem::Image {
                        id: 2,
                        source: Some(ImageSource {
                            uri: "asset:/pic.png".into(),
                            size: None,
                            scales: nlayer_core::ImageScales::NONE,
                        }),
                        resize_mode: ResizeMode::Contain,
                    },
                    MountItem::ViewStyle {
                        id: 2,
                        style: Box::new(parsed.view),
                    },
                ],
            ]
            .concat(),
        );

        let decoded =
            crate::image::load(&assets, "asset:/pic.png").expect("ImageIO decodes the PNG");
        assert_eq!((decoded.width, decoded.height), (4, 2), "the PNG's pixels");
        let generation = mounter
            .borrow()
            .slot(2)
            .and_then(|slot| slot.picture.as_ref())
            .expect("picture")
            .generation;
        mounter
            .borrow_mut()
            .finish_image(2, generation, 2.0, Ok(decoded));

        let mounter = mounter.borrow();
        let picture = mounter
            .slot(2)
            .and_then(|slot| slot.picture.as_ref())
            .expect("picture");
        assert_eq!(
            picture.natural,
            Some(Size::new(Points(2.0), Points(1.0))),
            "4×2 pixels at scale 2 are 2×1 points"
        );
        assert_eq!(picture.layer.contentsScale(), 2.0);
        assert!(picture.clip.masksToBounds(), "the picture is clipped");
        assert_eq!(picture.clip.cornerRadius(), 8.0, "the radius clips it");
        let fitted = picture.layer.frame();
        assert_eq!(
            (
                fitted.origin.x,
                fitted.origin.y,
                fitted.size.width,
                fitted.size.height
            ),
            (0.0, 8.0, 32.0, 16.0),
            "contain fits 2×1 into 32×32"
        );
        drop(mounter);
        let _ = std::fs::remove_dir_all(&assets);
    }

    fn a_text_block_carries_its_layout_and_runs() {
        let mounter = mounter();
        let value = "Hello";
        let attributed = Arc::new(AttributedText {
            text: value.into(),
            spans: vec![TextSpan {
                start: 0,
                end: value.encode_utf16().count() as u32,
                style: ResolvedTextStyle::default(),
            }],
            ..AttributedText::default()
        });
        let engine = crate::text::CoreTextEngine::default();
        let prepared = engine.prepare(&attributed);
        let layout = engine.layout(&prepared, size(px(200.0), px(40.0)));
        apply(
            &mounter,
            [
                child(2, ViewKind::Text, frame(0.0, 0.0, 200.0, 20.0)),
                vec![MountItem::Text {
                    id: 2,
                    text: Arc::clone(&attributed),
                    layout,
                    content: frame(0.0, 0.0, 200.0, 20.0),
                }],
            ]
            .concat(),
        );

        let scene = mounter.borrow().scene();
        assert_eq!(scene.texts(), ["Hello"]);
        let tree = scene.tree();
        assert_eq!(tree["children"][0]["text"], "Hello");
        assert_eq!(tree["children"][0]["spans"][0]["text"], "Hello");
        assert_eq!(tree["children"][0]["spans"][0]["fontSize"], 14.0);
        assert_eq!(tree["children"][0]["spans"][0]["fontWeight"], 400);
    }
}
