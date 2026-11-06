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

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Bounds, Points, Size};
use nlayer_core::style::{Color, TextStyle, ViewStyle};
use nlayer_core::{
    EventKind, EventMask, ImageSource, Interaction, KeyboardDismiss, KeyboardTaps, MountBatch,
    MountItem, NodeId, ROOT_ID, ResizeMode, ViewKind,
};
use nlayer_engine::{EngineControl, EventPayload};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CAGradientLayer, CALayer, CAShapeLayer};
use objc2_ui_kit::{
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer,
    UILongPressGestureRecognizer, UIPanGestureRecognizer, UIPinchGestureRecognizer,
    UIRefreshControl, UIRotationGestureRecognizer, UIScreen, UIScrollView, UITapGestureRecognizer,
    UIView,
};

use crate::input::{AreaInput, Field};
use crate::text_view::TextView;

mod a11y;
mod batch;
mod box_view;
mod editing;
mod gestures;
mod image;
mod nav;
mod paint;
mod relay;
mod scene_snap;
mod scroll;
mod session;
mod tree;
mod views;

#[cfg(feature = "host-tests")]
pub(crate) mod tests;

#[allow(unused_imports)]
pub(super) use a11y::*;
#[allow(unused_imports)]
pub(super) use batch::*;
#[allow(unused_imports)]
pub(super) use box_view::*;
#[allow(unused_imports)]
pub(super) use editing::*;
#[allow(unused_imports)]
pub(super) use gestures::*;
#[allow(unused_imports)]
pub(super) use image::*;
#[allow(unused_imports)]
pub(super) use nav::*;
#[allow(unused_imports)]
pub(super) use paint::*;
#[allow(unused_imports)]
pub(super) use relay::*;
#[allow(unused_imports)]
pub(super) use scene_snap::*;
#[allow(unused_imports)]
pub(super) use scroll::*;
#[allow(unused_imports)]
pub(super) use session::*;
#[allow(unused_imports)]
pub(super) use tree::*;
#[allow(unused_imports)]
pub(super) use views::*;

/// The scroll view touched inside a cooperating pan. Its starting offset
/// is consumed before the panel moves.
struct PanScroll {
    view: Retained<UIScrollView>,
    offset: f32,
}

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
    /// Bits the engine asked to hear (`PRESSED`, `HOVERED`). Focus is
    /// reported either way. Dropped with the slot.
    tracked: u16,
    /// Press, hover and focus this view holds for the engine. Focus is
    /// this view only, not the ancestors `focused` paints.
    bits: u16,
    press: Option<Retained<UILongPressGestureRecognizer>>,
    /// The view itself listens to `onPress` (not only a span inside it).
    own_press: bool,
    /// The view itself listens to `onLongPress`.
    own_long: bool,
    /// `delayLongPress` in milliseconds; `None` is 500.
    delay_long: Option<f32>,
    /// The press generation and whether this press already became long.
    long_generation: u64,
    long_fired: bool,
    /// Selection took this gesture: the release is not a press or an `href`.
    press_cancelled: bool,
    /// Listeners, including the scroll events. A scroll callback fires only
    /// a kind this contains.
    events: EventMask,
    /// A nested span listens to `onPress`.
    link_press: bool,
    hover: Option<Retained<UIHoverGestureRecognizer>>,
    /// Pan gesture recognizer.
    pan_recognizer: Option<Retained<UIPanGestureRecognizer>>,
    /// Pinch gesture recognizer.
    pinch_recognizer: Option<Retained<UIPinchGestureRecognizer>>,
    /// Rotation gesture recognizer.
    rotation_recognizer: Option<Retained<UIRotationGestureRecognizer>>,
    /// Pan binding: x and y animation nodes from `pan={{ x, y }}`.
    pan_nodes: (Option<u32>, Option<u32>),
    /// Pinch binding: scale animation node from `pinch={{ scale }}`.
    pinch_node: Option<u32>,
    /// Rotation binding: angle animation node from `rotation={{ angle }}`.
    rotation_node: Option<u32>,
    /// Gesture start values: [pan_x, pan_y, pinch_scale, rotation_angle].
    gesture_start: [f32; 4],
    /// Scroll view cooperation: a cooperating pan sees only scrolls between
    /// its view and the touch.
    pan_scroll: Option<PanScroll>,
    refresh_indicator: Option<f32>,
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
    /// Snap configuration for scroll views (from ScrollContent).
    snap_paging: bool,
    snap_interval: Option<f32>,
    snap_offsets: Option<Box<[f32]>>,
    snap_alignment: u8,
    snap_one_step: bool,
    snap_deceleration: f32,
    /// Pull-to-refresh control (only on scroll views).
    refresh_control: Option<Retained<UIRefreshControl>>,
    /// Whether refresh is currently active.
    refreshing: bool,
    /// Outer box-shadow layers, below the background.
    shadows: Vec<Retained<CALayer>>,
    /// Inset box-shadow shapes, above the gradients.
    insets: Option<(Retained<CALayer>, Vec<Retained<CAShapeLayer>>)>,
    /// Gradient backgrounds, above the background color.
    gradients: Vec<Retained<CAGradientLayer>>,
    /// Clips the children of a plain view whose outer shadow must escape.
    content: Option<Retained<UIView>>,
    /// Rounded clip of `content` when the four radii differ.
    content_mask: Option<Retained<CAShapeLayer>>,
    /// Holds outer shadows when the view layer itself is masked.
    shadow_host: Option<Retained<UIView>>,
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
    Stack(Box<crate::navigation::StackHost>),
    Screen(Box<crate::navigation::ScreenHost>),
    Modal(Box<crate::navigation::ModalHost>),
    HeaderItem(Retained<UIView>),
}

impl Native {
    fn view(&self) -> &UIView {
        match self {
            Native::View(view) | Native::Image(view) | Native::HeaderItem(view) => view,
            Native::Text(view) => view,
            Native::Scroll(view) => view,
            Native::Input(view) => view,
            Native::Area(area) => area.view(),
            Native::Stack(stack) => &stack.container,
            Native::Screen(screen) => &screen.view,
            Native::Modal(modal) => &modal.view,
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
    /// Whether the engine requested a frame via `request_frame`.
    frame_pending: bool,
    /// Navigation: screens hosted by stacks, modals and header items by screens.
    hosts: std::collections::HashMap<NodeId, NodeId>,
    /// Stacks whose screens changed.
    dirty_stacks: Vec<NodeId>,
    /// Screens whose header changed.
    dirty_headers: Vec<NodeId>,
    /// Modals to present or dismiss.
    dirty_modals: Vec<NodeId>,
    /// Stacks deferred while a transition runs.
    deferred_stacks: Vec<NodeId>,
    /// Gesture and animation bindings that arrived before their view was
    /// created: the core sets a prop before the batch creates the view.
    /// Applied when the view is created.
    pending_pan: std::collections::HashMap<NodeId, (Option<u32>, Option<u32>)>,
    pending_pinch: std::collections::HashMap<NodeId, Option<u32>>,
    pending_rotation: std::collections::HashMap<NodeId, Option<u32>>,
    pending_bindings:
        std::collections::HashMap<NodeId, Vec<(nlayer_core::animation::AnimationProp, u32)>>,
    /// Selectable views and the responder that owns each selection.
    areas: Vec<(NodeId, Retained<crate::selectable_view::AreaResponder>)>,
    /// URLs short presses on a `Text` `href` opened, in order. A pinned
    /// (`nlayer test`) app records them without leaving.
    opened_urls: Vec<String>,
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
            frame_pending: false,
            hosts: std::collections::HashMap::new(),
            dirty_stacks: Vec::new(),
            dirty_headers: Vec::new(),
            dirty_modals: Vec::new(),
            deferred_stacks: Vec::new(),
            pending_pan: std::collections::HashMap::new(),
            pending_pinch: std::collections::HashMap::new(),
            pending_rotation: std::collections::HashMap::new(),
            pending_bindings: std::collections::HashMap::new(),
            areas: Vec::new(),
            opened_urls: Vec::new(),
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
            tracked: 0,
            bits: 0,
            press: None,
            own_press: false,
            own_long: false,
            delay_long: None,
            long_generation: 0,
            long_fired: false,
            press_cancelled: false,
            events: EventMask::default(),
            link_press: false,
            hover: None,
            pan_recognizer: None,
            pinch_recognizer: None,
            rotation_recognizer: None,
            pan_nodes: (None, None),
            pinch_node: None,
            rotation_node: None,
            gesture_start: [0.0; 4],
            pan_scroll: None,
            refresh_indicator: None,
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
            snap_paging: false,
            snap_interval: None,
            snap_offsets: None,
            snap_alignment: 0,
            snap_one_step: false,
            snap_deceleration: 0.0,
            refresh_control: None,
            refreshing: false,
            shadows: Vec::new(),
            insets: None,
            gradients: Vec::new(),
            content: None,
            content_mask: None,
            shadow_host: None,
        }
    }
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

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Rc<std::cell::RefCell<Mounter>>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn current() -> Option<Rc<std::cell::RefCell<Mounter>>> {
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
