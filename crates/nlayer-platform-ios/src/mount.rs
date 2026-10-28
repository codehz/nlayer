//! Applies one mount batch to UIKit views.
//!
//! Extracted from `nsh-platform-ios/src/mount.rs` (`set_frame`, the transform
//! matrix, insert-below-sibling), `decor.rs` (the clipping image layer) and
//! `input.rs` (the single-line field). Animation and gestures beyond press
//! and hover are not applied.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use nlayer_core::geometry::{Bounds, Point, Points, Size};
use nlayer_core::style::{BorderStyle, Color, TextStyle, TransformOrigin, ViewStyle};
use nlayer_core::{
    EventKind, ImageSource, Interaction, MountBatch, MountItem, NodeId, ROOT_ID, ResizeMode,
    ViewKind,
};
use nlayer_engine::EngineControl;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_quartz_core::{CALayer, CAShapeLayer, CATransaction, CATransform3D, kCAGravityResize};
use objc2_ui_kit::{
    UIGestureRecognizerState, UIHoverGestureRecognizer, UILongPressGestureRecognizer, UIScreen,
    UIScrollView, UIScrollViewContentInsetAdjustmentBehavior, UIView,
};

use crate::input::Field;
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
    /// A nested span listens to `onPress`.
    link_press: bool,
    hover: Option<Retained<UIHoverGestureRecognizer>>,
    /// A dashed or dotted uniform border, above the children.
    dash: Option<Retained<CAShapeLayer>>,
    /// Per-side borders when the four sides are not one width and color.
    sides: [Option<Retained<CALayer>>; 4],
    /// The fitted bitmap, present only for an image view.
    picture: Option<Picture>,
    /// `testID`. Not drawn, and not an accessibility id.
    test_id: Option<Arc<str>>,
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
}

impl Native {
    fn view(&self) -> &UIView {
        match self {
            Native::View(view) | Native::Image(view) => view,
            Native::Text(view) => view,
            Native::Scroll(view) => view,
            Native::Input(view) => view,
        }
    }
}

/// Gesture target shared by every press and hover recognizer. The view's
/// tag is the node id.
struct RelayIvars {
    mounter: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<Mounter>>>,
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
            let Some(crate::input::TextUpdate::Report(text)) = field.edited() else {
                return;
            };
            let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
                return;
            };
            mounter.borrow().report_input(id, text);
        }

        #[unsafe(method(editingBegan:))]
        fn editing_began(&self, sender: &AnyObject) {
            self.focus_edit(sender, true);
        }

        #[unsafe(method(editingEnded:))]
        fn editing_ended(&self, sender: &AnyObject) {
            self.focus_edit(sender, false);
        }
    }

    unsafe impl NSObjectProtocol for Relay {}
);

impl Relay {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RelayIvars {
            mounter: std::cell::RefCell::new(std::rc::Weak::new()),
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

    fn focus_edit(&self, sender: &AnyObject, on: bool) {
        let Some(view) = sender.downcast_ref::<UIView>() else {
            return;
        };
        let id = view.tag() as u32;
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        if on {
            mounter.borrow_mut().begin_focus(id);
        } else {
            mounter.borrow_mut().end_focus(id);
        }
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
    /// Set by `nlayer test`. Replaces the window's size, safe area and
    /// keyboard height. A pin with a keyboard height ignores the software keyboard.
    pin: Option<crate::pin::Pin>,
    /// Software keyboard overlap, in points. An explicit pin height replaces it.
    keyboard: Points,
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
            pin: None,
            keyboard: Points::ZERO,
        }));
        CURRENT.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&mounter)));
        relay.bind(Rc::downgrade(&mounter));
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
        for item in batch.items {
            self.apply_item(item);
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
                if self.focused == Some(id) {
                    self.end_focus(id);
                }
                if let Some(slot) = self.take(id) {
                    slot.native.view().removeFromSuperview();
                }
            }
            MountItem::Frame { id, frame } => {
                if id == ROOT_ID {
                    return;
                }
                if let Some(view) = self.view(id) {
                    set_frame(view, frame);
                }
                self.repaint(id);
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
            MountItem::ScrollConfig { id, horizontal } => {
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                scroll.setAlwaysBounceHorizontal(horizontal);
                scroll.setAlwaysBounceVertical(!horizontal);
                scroll.setShowsHorizontalScrollIndicator(horizontal);
                scroll.setShowsVerticalScrollIndicator(!horizontal);
                scroll.setDirectionalLockEnabled(true);
            }
            MountItem::ScrollContent { id, size } => {
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                scroll.setContentSize(CGSize::new(
                    size.width.0 as CGFloat,
                    size.height.0 as CGFloat,
                ));
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
            MountItem::Command { id, command } => {
                let nlayer_core::Command::ScrollTo { x, y, animated } = command;
                let Some(Native::Scroll(scroll)) = self.native(id) else {
                    return;
                };
                scroll.setContentOffset_animated(
                    CGPoint::new(x.0 as CGFloat, y.0 as CGFloat),
                    animated,
                );
            }
            MountItem::Image {
                id,
                source,
                resize_mode,
            } => self.set_image(id, source, resize_mode),
            MountItem::Input { id, config } => {
                if let Some(Native::Input(field)) = self.native(id) {
                    field.configure(&config);
                }
                self.repaint(id);
            }
            MountItem::InputValue { id, text, count } => {
                if let Some(Native::Input(field)) = self.native(id) {
                    field.set_value(&text, count);
                }
            }
            MountItem::InputContent { id, content } => {
                if let Some(Native::Input(field)) = self.native(id) {
                    field.set_content(content);
                }
            }
            MountItem::TestId { id, test_id } => {
                if let Some(slot) = self.slot_mut(id) {
                    slot.test_id = test_id.clone();
                }
            }
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
                (Native::Scroll(scroll), None)
            }
            ViewKind::Image => {
                let (view, picture) = image_layers(self.mtm);
                (Native::Image(view), Some(picture))
            }
            ViewKind::Input => (Native::Input(Field::new(self.mtm, &self.relay)), None),
        };
        let view = native.view();
        view.setTag(id as isize);
        view.setOpaque(false);
        view.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        self.ensure(id);
        let mut slot = Slot::plain(native);
        slot.picture = picture;
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
            slot.own_press = events.contains(EventKind::Press);
        }
        self.sync_press(id);
        self.sync_hover(id);
    }

    fn sync_press(&mut self, id: NodeId) {
        let mtm = self.mtm;
        let relay = self.relay.clone();
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let want = slot.own_press || slot.link_press;
        let view = slot.native.view().retain();
        let input = matches!(slot.native, Native::Input(_));
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
        if let Some(Native::Input(field)) = self.native(id) {
            field.set_override_color(text_color);
        }
    }

    fn report_input(&self, id: NodeId, text: String) {
        if let Some(control) = &self.control {
            control.input_changed(id, text);
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

    pub(crate) fn field(&self, id: NodeId) -> Option<Retained<Field>> {
        match self.native(id)? {
            Native::Input(field) => Some(field.retain()),
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
                    Native::Input(_) => Kind::Input,
                },
                test_id: slot.test_id.as_ref().map(|id| id.to_string()),
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
            if let Native::Input(field) = &slot.native {
                node.input = Some(InputSnap {
                    value: field.shown(),
                    placeholder: field.placeholder_text(),
                    secure: field.secure(),
                    keyboard: field.keyboard_name().to_owned(),
                    disabled: field.disabled(),
                });
            }
            if let Native::Scroll(scroll) = &slot.native {
                let offset = scroll.contentOffset();
                let content = scroll.contentSize();
                node.scroll = Some(ScrollSnap {
                    horizontal: scroll.alwaysBounceHorizontal(),
                    x: offset.x as f32,
                    y: offset.y as f32,
                    content_w: content.width as f32,
                    content_h: content.height as f32,
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
            link_press: false,
            hover: None,
            dash: None,
            sides: [None, None, None, None],
            picture: None,
            test_id: None,
        }
    }
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
    apply_corners(&layer, radii, bounds);
    // A drag moves children with the content offset. The scroll viewport
    // clips them, the way `overflow: auto` does. `overflow` only clips a
    // plain view.
    layer.setMasksToBounds(style.overflow_hidden || scroll);
    apply_border(view, style, slot);
    layer.setTransform(transform_3d(style, bounds.size));
}

fn apply_corners(layer: &CALayer, radii: nlayer_core::style::CornerRadii, bounds: CGRect) {
    // SAFETY: clearing the mask, or installing a shape the layer retains.
    unsafe { layer.setMask(None) };
    if let Some(shared) = radii.shared() {
        layer.setCornerRadius(shared.0 as CGFloat);
        // MinXMinY is the top left in UIKit's y-down space.
        let mut mask: usize = 0;
        if radii.top_left > Points::ZERO {
            mask |= 1 << 0;
        }
        if radii.top_right > Points::ZERO {
            mask |= 1 << 1;
        }
        if radii.bottom_left > Points::ZERO {
            mask |= 1 << 2;
        }
        if radii.bottom_right > Points::ZERO {
            mask |= 1 << 3;
        }
        layer.setMaskedCorners(objc2_quartz_core::CACornerMask(mask));
        return;
    }
    layer.setCornerRadius(0.0);
    let path = rounded_path(bounds, radii);
    let mask = CAShapeLayer::layer();
    mask.setFrame(bounds);
    mask.setPath(Some(&path));
    unsafe { layer.setMask(Some(&mask)) };
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
        let pattern: &[CGFloat] = match borders.style {
            BorderStyle::Dotted => &[1.0, 2.0],
            _ => &[4.0, 3.0],
        };
        let dashes: Vec<_> = pattern
            .iter()
            .map(|n| objc2_foundation::NSNumber::numberWithDouble(*n))
            .collect();
        dash.setLineDashPattern(Some(&objc2_foundation::NSArray::from_retained_slice(
            &dashes,
        )));
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
fn set_frame(view: &UIView, frame: Bounds<Points>) {
    let origin = view.bounds().origin;
    view.setBounds(CGRect::new(
        origin,
        CGSize::new(
            frame.size.width.0 as CGFloat,
            frame.size.height.0 as CGFloat,
        ),
    ));
    view.setCenter(CGPoint::new(
        (frame.origin.x.0 + frame.size.width.0 / 2.0) as CGFloat,
        (frame.origin.y.0 + frame.size.height.0 / 2.0) as CGFloat,
    ));
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
}

impl Mounter {
    fn set_image(&mut self, id: NodeId, source: Option<ImageSource>, mode: ResizeMode) {
        let scale = screen_scale(self.mtm);
        let assets = self.assets.clone();
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
        if let Some((generation, Some(uri))) = reload {
            load_image(id, generation, uri, assets, scale);
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

fn load_image(id: NodeId, generation: u64, uri: Arc<str>, assets: PathBuf, scale: f32) {
    use dispatch2::{DispatchQueue, DispatchQueueGlobalPriority, GlobalQueueIdentifier};
    DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
        DispatchQueueGlobalPriority::Default,
    ))
    .exec_async(move || {
        let loaded = crate::image::load(&assets, &uri).map_err(|error| format!("{uri}: {error}"));
        DispatchQueue::main().exec_async(move || {
            let Some(mounter) = current() else {
                return;
            };
            mounter
                .borrow_mut()
                .finish_image(id, generation, scale, loaded);
        });
    });
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Rc<std::cell::RefCell<Mounter>>>> =
        const { std::cell::RefCell::new(None) };
}

fn current() -> Option<Rc<std::cell::RefCell<Mounter>>> {
    CURRENT.with(|slot| slot.borrow().clone())
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
        },
    )
}

/// Frames the picture where `objectFit` puts it, and clips that to the view.
fn place_image(view: &UIView, style: &ViewStyle, slot: &Slot) {
    let Some(picture) = &slot.picture else {
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
    quiet(|| {
        clip.setFrame(bounds);
        apply_corners(&clip, radii, clip.bounds());
        layer.setFrame(CGRect::new(
            CGPoint::new(fitted.origin.x.0 as CGFloat, fitted.origin.y.0 as CGFloat),
            CGSize::new(
                fitted.size.width.0 as CGFloat,
                fitted.size.height.0 as CGFloat,
            ),
        ));
    });
}

fn quiet<R>(f: impl FnOnce() -> R) -> R {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let result = f();
    CATransaction::commit();
    result
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
