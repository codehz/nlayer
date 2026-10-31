//! Native navigation: stacks of screens on `UINavigationController`, and
//! modals presented as view controllers.
//!
//! The core lays screens out at their stack's size (content below the
//! header) and modals at the size reported here; this module only
//! arranges controllers and reports what the user does natively (popping
//! with the back button or a swipe, swiping a sheet down).
//!
//! UIKit asks some of this synchronously (may the swipe begin, may the back
//! button pop, may the sheet close): the answers come from props already
//! mounted (`gesture_enabled`, `prevent_back`, `close_on_swipe`, `close_on_escape`),
//! never from JS.

use std::cell::{Cell, RefCell};

use block2::RcBlock;
use nlayer_core::geometry::{Points, Size};
use nlayer_core::{
    Color, EventKind, ModalAnimation, ModalConfig, ModalPresentation, NodeId, ScreenAnimation,
    ScreenConfig,
};
use nlayer_engine::{EngineControl, EventPayload};
use objc2::{
    DeclaredClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    rc::Weak, runtime::AnyClass, runtime::AnyObject, runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use objc2_quartz_core::{CAMediaTiming, CATransition};
use objc2_ui_kit::{
    UIAdaptivePresentationControllerDelegate, UIBarButtonItem, UIBehavioralStyle, UIColor,
    UIEdgeInsets, UIGestureRecognizer, UIGestureRecognizerDelegate, UIModalPresentationStyle,
    UIModalTransitionStyle, UINavigationBar, UINavigationBarAppearance, UINavigationController,
    UINavigationControllerDelegate, UINavigationItem, UIPresentationController, UIResponder,
    UIView, UIViewAutoresizing, UIViewController,
};

type EdgeInsets = nlayer_core::geometry::Edges<Points>;

fn insets_payload(i: EdgeInsets) -> EventPayload {
    EventPayload::Insets {
        top: i.top.0,
        right: i.right.0,
        bottom: i.bottom.0,
        left: i.left.0,
    }
}

/// `RequestClose`'s payload: what the user did (`"cancel"`, `"swipe"`).
fn close_reason(reason: &str) -> EventPayload {
    EventPayload::CloseReason(reason.to_owned())
}

/// A `UIKeyCommand` for Escape that calls `nlayerEscape:`.
fn escape_command() -> Retained<AnyObject> {
    let class = AnyClass::get(c"UIKeyCommand").expect("UIKeyCommand");
    // SAFETY: a UIKit constant, and UIKeyCommand's documented factory.
    unsafe {
        msg_send![
            class,
            keyCommandWithInput: objc2_ui_kit::UIKeyInputEscape,
            modifierFlags: 0isize,
            action: objc2::sel!(nlayerEscape:)
        ]
    }
}

fn ui_color(c: Color) -> Retained<UIColor> {
    let (r, g, b, a) = c.to_f64();
    UIColor::colorWithRed_green_blue_alpha(r, g, b, a)
}

pub(crate) struct ScreenIvars {
    id: NodeId,
    control: Option<EngineControl>,
    /// Height of the header area the core laid the content out below.
    padded: Cell<f32>,
    /// The core lays the content out below the header: it gets the header's
    /// real height (`set_screen_header`).
    measure: Cell<bool>,
    /// The header height last reported to the core.
    measured: Cell<Option<f32>>,
    insets: Cell<Option<EdgeInsets>>,
}

define_class!(
    /// The controller of one screen; its view hosts the screen's root view.
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ScreenIvars]
    pub(crate) struct ScreenController;

    impl ScreenController {
        #[unsafe(method(viewDidLayoutSubviews))]
        fn view_did_layout_subviews(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewDidLayoutSubviews] };
            self.report_insets();
        }

        /// The stack correcting what UIKit granted a screen beside a side
        /// bar changes its safe area without moving anything in it: the
        /// module hears the corrected insets from here, not a layout pass.
        #[unsafe(method(viewSafeAreaInsetsDidChange))]
        fn view_safe_area_insets_did_change(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewSafeAreaInsetsDidChange] };
            self.report_insets();
        }
    }
);

impl ScreenController {
    /// Reports the top of the view's safe area (the bottom of the header UIKit
    /// shows, wherever it put it) as the header the content starts below.
    /// The core's estimate (the bar's height plus the status bar) is wrong
    /// on Mac Catalyst, where the bar can sit in the window's title bar.
    fn report_header(&self, view: &UIView, top: f32) {
        let ivars = self.ivars();
        let header = if !ivars.measure.get() {
            None
        } else if view.window().is_some()
            && let Some(nav) = self.navigationController()
            && !nav.isNavigationBarHidden()
        {
            Some(top)
        } else {
            // Off screen or between headers: keep what was measured.
            return;
        };
        if ivars.measured.replace(header) != header
            && let Some(control) = &ivars.control
        {
            control.set_screen_header(ivars.id, header.map(Points));
        }
    }

    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: NodeId,
        control: Option<EngineControl>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ScreenIvars {
            id,
            control,
            padded: Cell::new(0.0),
            measure: Cell::new(false),
            measured: Cell::new(None),
            insets: Cell::new(None),
        });
        // SAFETY: UIViewController's designated initializer.
        unsafe {
            msg_send![super(this), initWithNibName: Option::<&NSString>::None, bundle: Option::<&NSObject>::None]
        }
    }

    pub(crate) fn screen_id(&self) -> NodeId {
        self.ivars().id
    }

    /// The screen's insets not covered by system UI or its header, for
    /// `safeAreaInsets()` inside it; and where its content starts below
    /// the header.
    pub(crate) fn report_insets(&self) {
        let Some(view) = self.view() else { return };
        let s = view.safeAreaInsets();
        self.report_header(&view, s.top as f32);
        let insets = EdgeInsets {
            top: Points((s.top as f32 - self.ivars().padded.get()).max(0.0)),
            right: Points(s.right as f32),
            bottom: Points(s.bottom as f32),
            left: Points(s.left as f32),
        };
        if self.ivars().insets.replace(Some(insets)) != Some(insets)
            && let Some(control) = &self.ivars().control
        {
            let id = self.ivars().id;
            control.dispatch_event(id, EventKind::Insets, insets_payload(insets));
        }
    }

    #[cfg(feature = "host-tests")]
    pub(crate) fn reported_insets(&self) -> Option<(f32, f32, f32, f32)> {
        self.ivars()
            .insets
            .get()
            .map(|i| (i.top.0, i.right.0, i.bottom.0, i.left.0))
    }
}

fn first_responder(view: &UIView) -> Option<Retained<UIView>> {
    if view.isFirstResponder() {
        return Some(view.retain());
    }
    view.subviews()
        .iter()
        .find_map(|child| first_responder(&child))
}

pub(crate) struct ModalIvars {
    id: NodeId,
    control: Option<EngineControl>,
    reported: Cell<Option<(objc2_core_foundation::CGSize, EdgeInsets)>>,
    /// `closeOnSwipe`: a swipe down closes the sheet natively.
    close_on_swipe: Cell<bool>,
    /// `closeOnEscape`: the Escape key (and VoiceOver's escape gesture)
    /// closes it natively.
    close_on_escape: Cell<bool>,
    /// It appears and leaves with an animation.
    animated: Cell<bool>,
    /// The user closed it natively; the app has not hidden it yet.
    closed: Cell<bool>,
}

define_class!(
    /// The controller a modal is presented with.
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ModalIvars]
    pub(crate) struct ModalController;

    unsafe impl NSObjectProtocol for ModalController {}
    unsafe impl UIAdaptivePresentationControllerDelegate for ModalController {
        /// Answered from `closeOnSwipe`: close natively (reported when
        /// done), or let `open` decide (a swipe down only asks).
        #[unsafe(method(presentationControllerShouldDismiss:))]
        fn should_dismiss(&self, _controller: &UIPresentationController) -> bool {
            self.ivars().close_on_swipe.get()
        }

        /// The user swiped a `closeOnSwipe` sheet away.
        #[unsafe(method(presentationControllerDidDismiss:))]
        fn did_dismiss(&self, _controller: &UIPresentationController) {
            let ivars = self.ivars();
            ivars.closed.set(true);
            if let Some(control) = &ivars.control {
                control.dispatch_event(ivars.id, EventKind::RequestClose, close_reason("swipe"));
                control.dispatch_event(ivars.id, EventKind::Dismiss, EventPayload::None);
            }
        }

        /// A swipe down on a sheet that stays (`closeOnSwipe` false).
        #[unsafe(method(presentationControllerDidAttemptToDismiss:))]
        fn did_attempt_to_dismiss(&self, _controller: &UIPresentationController) {
            if let Some(control) = &self.ivars().control {
                let id = self.ivars().id;
                control.dispatch_event(id, EventKind::RequestClose, close_reason("swipe"));
            }
        }
    }

    impl ModalController {
        #[unsafe(method(viewDidLayoutSubviews))]
        fn view_did_layout_subviews(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewDidLayoutSubviews] };
            self.report_size();
        }

        #[unsafe(method(viewDidAppear:))]
        fn view_did_appear(&self, animated: bool) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewDidAppear: animated] };
            if let Some(control) = &self.ivars().control {
                let id = self.ivars().id;
                control.dispatch_event(id, EventKind::Show, EventPayload::None);
            }
        }

        /// Escape on a hardware keyboard.
        #[unsafe(method_id(keyCommands))]
        fn key_commands(&self) -> Option<Retained<NSArray<AnyObject>>> {
            // SAFETY: calling the superclass implementation.
            let inherited: Option<Retained<NSArray<AnyObject>>> =
                unsafe { msg_send![super(self), keyCommands] };
            let mut commands = inherited.map(|a| a.to_vec()).unwrap_or_default();
            commands.push(escape_command());
            Some(NSArray::from_retained_slice(&commands))
        }

        #[unsafe(method(nlayerEscape:))]
        fn escape(&self, _sender: Option<&AnyObject>) {
            self.cancel();
        }

        /// VoiceOver's escape gesture (a two-finger scrub).
        #[unsafe(method(accessibilityPerformEscape))]
        fn accessibility_perform_escape(&self) -> bool {
            self.cancel();
            true
        }
    }
);

impl ModalController {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: NodeId,
        control: Option<EngineControl>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ModalIvars {
            id,
            control,
            reported: Cell::new(None),
            close_on_swipe: Cell::new(false),
            close_on_escape: Cell::new(false),
            animated: Cell::new(true),
            closed: Cell::new(false),
        });
        // SAFETY: UIViewController's designated initializer.
        unsafe {
            msg_send![super(this), initWithNibName: Option::<&NSString>::None, bundle: Option::<&NSObject>::None]
        }
    }

    /// Applies the close flags (also while presented).
    pub(crate) fn set_flags(&self, c: &ModalConfig) {
        let ivars = self.ivars();
        ivars.close_on_swipe.set(c.close_on_swipe);
        ivars.close_on_escape.set(c.close_on_escape);
        ivars.animated.set(c.animation != ModalAnimation::None);
        // A sheet that stays only reports the swipe
        // (`presentationControllerDidAttemptToDismiss:`).
        self.setModalInPresentation(!c.close_on_swipe);
    }

    /// Escape: reported as `cancel`, and closed natively with
    /// `closeOnEscape` (reported when done).
    fn cancel(&self) {
        let ivars = self.ivars();
        // Not presented, or under another modal (the top one handles it).
        if ivars.closed.get()
            || self.presentingViewController().is_none()
            || self.presentedViewController().is_some()
        {
            return;
        }
        let (id, control) = (ivars.id, ivars.control.clone());
        if let Some(control) = &control {
            control.dispatch_event(id, EventKind::RequestClose, close_reason("cancel"));
        }
        if !ivars.close_on_escape.get() {
            return;
        }
        ivars.closed.set(true);
        let block = RcBlock::new(move || {
            if let Some(control) = &control {
                control.dispatch_event(id, EventKind::Dismiss, EventPayload::None);
            }
        });
        self.dismissViewControllerAnimated_completion(ivars.animated.get(), Some(&block));
    }

    /// Lays the modal's content out at the size it is presented at.
    fn report_size(&self) {
        let Some(view) = self.view() else { return };
        let size = view.bounds().size;
        let s = view.safeAreaInsets();
        let insets = EdgeInsets {
            top: Points(s.top as f32),
            right: Points(s.right as f32),
            bottom: Points(s.bottom as f32),
            left: Points(s.left as f32),
        };
        if size.width <= 0.0
            || self.ivars().reported.replace(Some((size, insets))) == Some((size, insets))
        {
            return;
        }
        let ivars = self.ivars();
        if let Some(control) = &ivars.control {
            control.set_root_size(
                ivars.id,
                Size::new(Points(size.width as f32), Points(size.height as f32)),
                insets,
            );
            control.dispatch_event(ivars.id, EventKind::Insets, insets_payload(insets));
        }
    }
}

pub(crate) struct NavIvars {
    stack: NodeId,
    applying: Cell<bool>,
}

/// A drag that starts at the screen's edge belongs to the stack: the top
/// screen's scroll views wait for the back gesture to fail before they pan,
/// so a swipe at the edge goes back instead of scrolling the page. The
/// gesture fails at once for a drag that is not at the edge. Scroll views
/// mounted after the screen's last stack change get it with the next one.
fn defer_to_back_swipe(nav: &UINavigationController) {
    let Some(pop) = nav.interactivePopGestureRecognizer() else {
        return;
    };
    let Some(view) = nav.topViewController().and_then(|c| c.view()) else {
        return;
    };
    let mut pending = vec![view];
    while let Some(view) = pending.pop() {
        if let Some(scroll) = view.downcast_ref::<objc2_ui_kit::UIScrollView>() {
            scroll
                .panGestureRecognizer()
                .requireGestureRecognizerToFail(&pop);
        }
        pending.extend(view.subviews().iter());
    }
}

/// Gives every screen whose view UIKit left empty its parent's bounds.
///
/// UIKit sizes a pushed controller's view from the navigation view, which
/// is still empty when the stack mounts: autoresizing cannot grow a zero
/// size, so the view keeps an empty frame even once the navigation view
/// has one. The screen's content is not clipped, so it draws correctly,
/// but an empty view answers `pointInside:` with false, which ends
/// `hitTest:` there, and the accessibility snapshot UI tests read drops
/// the whole subtree under it: the screen is visible and nothing in it
/// can be touched or found.
fn size_screens(nav: &UINavigationController) {
    for controller in nav.viewControllers().iter() {
        if !controller.isViewLoaded() {
            continue;
        }
        let Some(view) = controller.view() else {
            continue;
        };
        let Some(parent) = view.superview() else {
            continue;
        };
        let size = view.bounds().size;
        let bounds = parent.bounds();
        if (size.width <= 0.0 || size.height <= 0.0)
            && bounds.size.width > 0.0
            && bounds.size.height > 0.0
        {
            view.setFrame(bounds);
        }
    }
}

/// Holds a screen's horizontal safe area to the part of it its stack covers.
///
/// A bar the device keeps along one side of the screen (the iPhone Duo's
/// vertical bar) is an inset of the window, and `UINavigationController`
/// hands that inset to the screens it shows without asking whether it still
/// reaches the bar: a stack laid out in a column that stops well short of it
/// passes the bar's whole width down as trailing safe area anyway. Content
/// stays where it belongs (the core lays it out from the insets the screen
/// reports, and a scroll view here adjusts nothing), but UIKit parks scroll
/// indicators that far in from the edge, and `safeAreaInsets()` reads that
/// wide inside the module.
///
/// What a screen is owed on a side is how much of the window's unsafe strip
/// there the stack's view really covers; a negative `additionalSafeAreaInsets`
/// takes the rest back. Both numbers come from the window and the stack's
/// frame, never from what a screen's safe area currently is. Recovering the
/// inset UIKit granted by subtracting the compensation already applied is the
/// tempting shortcut, and it cannot heal. UIKit floors a safe area at zero, so
/// once a compensation overshoots (the bar changes side and yesterday's value
/// stays behind) that subtraction hands the overshoot straight back, and every
/// later pass arrives at the same wrong number again. Taken from geometry and
/// written to both sides every time, it is idempotent, and leaves nothing
/// behind on the side the bar left.
fn clip_side_safe_area(nav: &UINavigationController) {
    let Some(view) = nav.viewIfLoaded() else {
        return;
    };
    // Out of the window there is no bar to measure against; the layout pass
    // that follows the stack going back in works this out again.
    let Some(window) = view.window() else {
        return;
    };
    let safe = window.safeAreaInsets();
    let bounds = window.bounds();
    let frame = view.convertRect_toView(view.bounds(), Some(&window));
    let (stack_min, stack_max) = (frame.min().x, frame.max().x);
    let covered = |min: objc2_core_foundation::CGFloat, max: objc2_core_foundation::CGFloat| {
        (stack_max.min(max) - stack_min.max(min)).max(0.0)
    };
    let left = covered(bounds.min().x, bounds.min().x + safe.left) - safe.left;
    let right = covered(bounds.max().x - safe.right, bounds.max().x) - safe.right;
    for controller in nav.viewControllers().iter() {
        let current = controller.additionalSafeAreaInsets();
        if current.left == left && current.right == right {
            continue;
        }
        controller.setAdditionalSafeAreaInsets(UIEdgeInsets {
            left,
            right,
            ..current
        });
    }
}

define_class!(
    /// A stack's navigation controller: the back button asks it before
    /// popping (`navigationBar:shouldPopItem:`), answered from `prevent_back`.
    #[unsafe(super(UINavigationController, UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = NavIvars]
    pub(crate) struct NavController;

    impl NavController {
        /// The header's back button: a screen listening to `onBackPress`
        /// hears of it, and one with `prevent_back` stays.
        #[unsafe(method(navigationBar:shouldPopItem:))]
        fn should_pop_item(&self, bar: &UINavigationBar, item: &UINavigationItem) -> bool {
            let ivars = self.ivars();
            // The app's own pops, and the end of an interactive swipe
            // (`gesture_enabled`, a transition already running), are no press.
            let press = !ivars.applying.get() && self.transitionCoordinator().is_none();
            let mut stay = false;
            if press
                && let Some(m) = crate::mount::current()
                && let Ok(m) = m.try_borrow()
                && let Some((screen, listens, prevent)) = m.stack_back_press(ivars.stack)
            {
                if listens {
                    m.dispatch(screen, EventKind::BackRequest);
                }
                stay = prevent;
            }
            if stay {
                false
            } else {
                // SAFETY: calling the superclass implementation.
                unsafe { msg_send![super(self), navigationBar: bar, shouldPopItem: item] }
            }
        }

        /// Whatever a layout pass left empty (a stack that grew after it
        /// mounted, a window that changed size) is sized here.
        #[unsafe(method(viewDidLayoutSubviews))]
        fn view_did_layout_subviews(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewDidLayoutSubviews] };
            size_screens(self);
            clip_side_safe_area(self);
        }

        /// A side bar arriving, leaving or changing side moves no view of
        /// the stack, so the screens' compensation is worked out here too.
        #[unsafe(method(viewSafeAreaInsetsDidChange))]
        fn view_safe_area_insets_did_change(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), viewSafeAreaInsetsDidChange] };
            clip_side_safe_area(self);
        }
    }
);

impl NavController {
    pub(crate) fn new(mtm: MainThreadMarker, stack: NodeId) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavIvars {
            stack,
            applying: Cell::new(false),
        });
        // SAFETY: UIViewController's designated initializer.
        unsafe {
            msg_send![super(this), initWithNibName: Option::<&NSString>::None, bundle: Option::<&NSObject>::None]
        }
    }
}

pub(crate) struct StackDelegateIvars {
    stack: NodeId,
    /// An accepted gesture ended editing; return it if the same page wins.
    editor: RefCell<Option<Weak<UIView>>>,
}

define_class!(
    /// Follows the navigation controller of one stack: per-screen header
    /// visibility, swipe-back permission, and screens the user popped.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = StackDelegateIvars]
    pub(crate) struct StackDelegate;

    unsafe impl NSObjectProtocol for StackDelegate {}
    unsafe impl UINavigationControllerDelegate for StackDelegate {
        #[unsafe(method(navigationController:willShowViewController:animated:))]
        fn will_show(
            &self,
            nav: &UINavigationController,
            controller: &UIViewController,
            animated: bool,
        ) {
            self.with_mounter(|m| m.stack_will_show(nav, controller, animated));
        }

        #[unsafe(method(navigationController:didShowViewController:animated:))]
        fn did_show(
            &self,
            nav: &UINavigationController,
            controller: &UIViewController,
            _animated: bool,
        ) {
            if let Some(editor) = self.ivars().editor.borrow_mut().take()
                && let Some(editor) = editor.load()
                && let Some(view) = controller.view()
                && editor.isDescendantOfView(&view)
                && let Some(window) = editor.window()
                && first_responder(&window).is_none()
            {
                // The swipe was cancelled. Another control's focus wins.
                editor.becomeFirstResponder();
            }
            // An animated change of screens is deferred to a layout pass of
            // the navigation view, which puts the screen that arrives in the
            // hierarchy after `reconcile` has sized what was there: the one
            // it brought in is only reachable here.
            size_screens(nav);
            defer_to_back_swipe(nav);
            let stack = self.ivars().stack;
            self.with_mounter(|m| m.stack_did_show(stack));
            // UIKit installs its own pop delegate while loading the navigation
            // view. Restore ours after showing a screen so hidden headers still
            // use the mounted swipe-back policy.
            install_pop_delegate(nav, self);
        }
    }

    unsafe impl UIGestureRecognizerDelegate for StackDelegate {
        /// Swipe back: only with something to go back to, if the top
        /// screen allows it.
        #[unsafe(method(gestureRecognizerShouldBegin:))]
        fn should_begin(&self, recognizer: &UIGestureRecognizer) -> bool {
            let stack = self.ivars().stack;
            let allowed = match crate::mount::current() {
                Some(m) => match m.try_borrow() {
                    Ok(m) => m.stack_can_pop(stack),
                    Err(_) => false,
                },
                None => false,
            };
            if allowed
                && let Some(view) = recognizer.view()
                && let Some(editor) = first_responder(&view)
            {
                // The outgoing view stays attached until the pop ends.
                // Resign now, after the permission/root/transition guards,
                // so the keyboard hides while the finger is still down.
                self.ivars()
                    .editor
                    .borrow_mut()
                    .replace(Weak::from_retained(&editor));
                view.endEditing(true);
            }
            allowed
        }
    }
);

/// Gives the back swipe's recognizers this stack's permission delegate.
/// UIKit installs its own delegates while loading the navigation view, so
/// this runs after the view exists and again after every transition. On
/// iOS 26 an edge drag can start on the content recognizer, whose default
/// delegate refuses to pop under a hidden navigation bar; both therefore
/// carry the mounted `popOnSwipe` / `popOnBack` policy.
fn install_pop_delegate(nav: &UINavigationController, delegate: &StackDelegate) {
    if let Some(pop) = nav.interactivePopGestureRecognizer() {
        pop.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    }
    if nav.respondsToSelector(objc2::sel!(interactiveContentPopGestureRecognizer))
        && let Some(pop) = nav.interactiveContentPopGestureRecognizer()
    {
        pop.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    }
}

impl StackDelegate {
    fn with_mounter(&self, f: impl FnOnce(&mut crate::mount::Mounter)) {
        if let Some(m) = crate::mount::current()
            && let Ok(mut m) = m.try_borrow_mut()
        {
            f(&mut m);
        }
    }

    pub(crate) fn new(mtm: MainThreadMarker, stack: NodeId) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StackDelegateIvars {
            stack,
            editor: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// A navigation stack.
pub(crate) struct StackHost {
    /// Placed by the core; hosts the navigation controller's view.
    pub(crate) container: Retained<UIView>,
    pub(crate) nav: Retained<NavController>,
    _delegate: Retained<StackDelegate>,
    /// The screens, bottom to top, as the app has them.
    pub(crate) screens: Vec<NodeId>,
    /// Screens the user popped natively, until the app removes them.
    pub(crate) popped: Vec<NodeId>,
    /// The screens last given to the navigation controller (changes wait
    /// while a transition runs).
    pub(crate) applied: RefCell<Vec<NodeId>>,
    /// The controller is a child of the surface's controller.
    contained: bool,
}

impl StackHost {
    pub(crate) fn new(mtm: MainThreadMarker, id: NodeId) -> Self {
        let container = UIView::new(mtm);
        let nav = NavController::new(mtm, id);
        let delegate = StackDelegate::new(mtm, id);
        // SAFETY: the delegate lives as long as the stack.
        unsafe { nav.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        keep_in_window(&nav.navigationBar());
        if let Some(view) = nav.view() {
            view.setAutoresizingMask(
                UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
            );
            view.setFrame(container.bounds());
            container.addSubview(&view);
        }
        // Loading the navigation view installs UIKit's gesture delegates.
        // Apply our permission delegate afterwards.
        install_pop_delegate(&nav, &delegate);
        Self {
            container,
            nav,
            _delegate: delegate,
            screens: Vec::new(),
            popped: Vec::new(),
            applied: Default::default(),
            contained: false,
        }
    }

    /// Gives the navigation controller's view the container's size. UIKit
    /// does not grow a zero-sized view by autoresizing when it is added.
    pub(crate) fn layout(&self) {
        if let Some(view) = self.nav.view() {
            view.setFrame(self.container.bounds());
        }
    }

    /// Makes the navigation controller a child of the controller showing
    /// the stack (appearance callbacks, safe areas, rotation).
    pub(crate) fn contain(&mut self) {
        if self.contained {
            return;
        }
        let Some(parent) = controller_of(&self.container) else {
            return;
        };
        parent.addChildViewController(&self.nav);
        self.nav.didMoveToParentViewController(Some(&parent));
        self.contained = true;
    }

    /// Shows the app's screens: pushes, pops or replaces as needed.
    /// `leaving` is the transition of the screen on top until now.
    pub(crate) fn reconcile(
        &self,
        screens: &[(NodeId, Retained<ScreenController>, ScreenAnimation)],
        leaving: Option<ScreenAnimation>,
    ) {
        let desired: Vec<Retained<UIViewController>> = screens
            .iter()
            .map(|(_, c, _)| Retained::into_super(c.clone()))
            .collect();
        let current = self.nav.viewControllers();
        let same = current.count() == desired.len()
            && desired
                .iter()
                .zip(current.iter())
                .all(|(a, b)| std::ptr::eq::<UIViewController>(&**a, &*b));
        if same {
            return;
        }
        let top_changed = match (current.lastObject(), desired.last()) {
            (Some(a), Some(b)) => !std::ptr::eq::<UIViewController>(&*a, &**b),
            _ => true,
        };
        // The animation of the screen that appears (push) or leaves (pop).
        let pushing = desired.len() >= current.count();
        let animation = if pushing {
            screens.last().map(|s| s.2)
        } else {
            leaving
        }
        .unwrap_or_default();
        let animated = top_changed && current.count() > 0 && animation == ScreenAnimation::Default;
        if top_changed && current.count() > 0 && animation == ScreenAnimation::Fade {
            let fade = CATransition::new();
            fade.setDuration(0.25);
            if let Some(view) = self.nav.view() {
                view.layer().addAnimation_forKey(&fade, None);
            }
        }
        let array = NSArray::from_retained_slice(&desired);
        *self.applied.borrow_mut() = screens.iter().map(|(id, _, _)| *id).collect();
        self.nav.ivars().applying.set(true);
        self.nav.setViewControllers_animated(&array, animated);
        self.nav.ivars().applying.set(false);

        // A stack whose size is settled is never laid out again for the
        // screens it is given, so nothing would size a screen UIKit left
        // empty until something unrelated (a rotation, a change of
        // appearance) forces a layout pass.
        size_screens(&self.nav);
        defer_to_back_swipe(&self.nav);
    }
}

impl Drop for StackHost {
    fn drop(&mut self) {
        self.nav.view().map(|view| view.removeFromSuperview());
        self.nav.removeFromParentViewController();
    }
}

/// One screen.
pub(crate) struct ScreenHost {
    pub(crate) controller: Retained<ScreenController>,
    /// The screen's root view, placed by the core.
    pub(crate) view: Retained<UIView>,
    pub(crate) config: ScreenConfig,
}

impl ScreenHost {
    pub(crate) fn new(mtm: MainThreadMarker, id: NodeId, control: Option<EngineControl>) -> Self {
        let controller = ScreenController::new(mtm, id, control);
        let view = UIView::new(mtm);
        if let Some(root) = controller.view() {
            root.setBackgroundColor(Some(&UIColor::systemBackgroundColor()));
            root.addSubview(&view);
        }
        Self {
            controller,
            view,
            config: ScreenConfig::default(),
        }
    }

    /// Applies the header options; `item` returns the view of a header item.
    /// The bar is UIKit's: a `header` slot (`header_view`, the whole bar
    /// where nlayer draws it) is never placed, so it stays out of sight.
    pub(crate) fn apply_header(
        &self,
        item: impl Fn(NodeId) -> Option<Retained<UIView>>,
        mtm: MainThreadMarker,
    ) {
        let c = &self.config;
        let nav_item = self.controller.navigationItem();
        nav_item.setTitle(c.title.as_deref().map(NSString::from_str).as_deref());
        let title_view = c.title_view.and_then(&item);
        nav_item.setTitleView(title_view.as_deref());
        let bar_item = |id: Option<NodeId>| {
            id.and_then(&item)
                .map(|v| UIBarButtonItem::initWithCustomView(UIBarButtonItem::alloc(mtm), &v))
        };
        nav_item.setLeftBarButtonItem(bar_item(c.left).as_deref());
        nav_item.setLeftItemsSupplementBackButton(true);
        nav_item.setRightBarButtonItem(bar_item(c.right).as_deref());
        nav_item.setHidesBackButton(!c.back_visible);
        let appearance = header_appearance(mtm, c);
        nav_item.setStandardAppearance(appearance.as_deref());
        nav_item.setScrollEdgeAppearance(appearance.as_deref());
        nav_item.setCompactAppearance(appearance.as_deref());
        // The bar of the screen on top, too (Catalyst does not always take
        // the item's appearance).
        if let Some(nav) = self.controller.navigationController()
            && nav
                .topViewController()
                .is_some_and(|top| std::ptr::eq::<UIViewController>(&*top, &**self.controller))
        {
            style_bar(&nav.navigationBar(), c);
        }
        let padded = if c.header_transparent {
            0.0
        } else {
            c.header_height.0
        };
        let ivars = self.controller.ivars();
        ivars.padded.set(padded);
        ivars.measure.set(c.header_visible && !c.header_transparent);
        self.controller.report_insets();
    }
}

pub(crate) fn keep_in_window(bar: &UINavigationBar) {
    // `preferredBehavioralStyle` is iOS 16+.
    if bar.respondsToSelector(objc2::sel!(setPreferredBehavioralStyle:)) {
        bar.setPreferredBehavioralStyle(UIBehavioralStyle::Pad);
    }
}

/// The header's appearance from the screen's options (`None`: UIKit's).
fn header_appearance(
    mtm: MainThreadMarker,
    c: &ScreenConfig,
) -> Option<Retained<UINavigationBarAppearance>> {
    if c.background_color.is_none() && c.title_color.is_none() && !c.header_transparent {
        return None;
    }
    let appearance = UINavigationBarAppearance::new(mtm);
    if c.header_transparent {
        appearance.configureWithTransparentBackground();
    } else if let Some(bg) = c.background_color {
        appearance.configureWithOpaqueBackground();
        appearance.setBackgroundColor(Some(&ui_color(bg)));
    } else {
        appearance.configureWithDefaultBackground();
    }
    if let Some(color) = c.title_color {
        let attributes = objc2_foundation::NSDictionary::from_retained_objects(
            // SAFETY: an immutable UIKit constant.
            &[unsafe { objc2_ui_kit::NSForegroundColorAttributeName }],
            &[Retained::into_super(Retained::into_super(ui_color(color)))],
        );
        // SAFETY: a valid title attributes dictionary.
        unsafe {
            appearance.setTitleTextAttributes(&attributes);
        }
    }
    Some(appearance)
}

/// Styles a stack's navigation bar for the screen it shows: the screen's
/// navigation item carries the same appearance, but Mac Catalyst does not
/// always apply it, so the bar gets it as well.
pub(crate) fn style_bar(bar: &UINavigationBar, c: &ScreenConfig) {
    let mtm = bar.mtm();
    let tint = c.tint_color.map(ui_color);
    // SAFETY: a plain setter; `None` restores the default tint.
    unsafe { bar.setTintColor(tint.as_deref()) };
    match header_appearance(mtm, c) {
        Some(appearance) => {
            bar.setStandardAppearance(&appearance);
            bar.setScrollEdgeAppearance(Some(&appearance));
            bar.setCompactAppearance(Some(&appearance));
        }
        None => {
            let appearance = UINavigationBarAppearance::new(mtm);
            bar.setStandardAppearance(&appearance);
            bar.setScrollEdgeAppearance(None);
            bar.setCompactAppearance(None);
        }
    }
}

/// One modal.
pub(crate) struct ModalHost {
    pub(crate) controller: Retained<ModalController>,
    /// The modal's root view, placed by the core.
    pub(crate) view: Retained<UIView>,
    pub(crate) config: ModalConfig,
    pub(crate) presented: bool,
}

impl ModalHost {
    pub(crate) fn new(mtm: MainThreadMarker, id: NodeId, control: Option<EngineControl>) -> Self {
        let controller = ModalController::new(mtm, id, control);
        let view = UIView::new(mtm);
        if let Some(root) = controller.view() {
            root.addSubview(&view);
        }
        Self {
            controller,
            view,
            config: ModalConfig::default(),
            presented: false,
        }
    }

    /// Presents the modal over the controller showing `from`.
    pub(crate) fn present(&mut self, from: &UIView) {
        if self.presented && !self.controller.ivars().closed.get() {
            return;
        }
        self.controller.ivars().closed.set(false);
        let Some(mut presenter) = controller_of(from) else {
            return log::warn!("cannot present a modal: no view controller shows the app");
        };
        while let Some(next) = presenter.presentedViewController() {
            presenter = next;
        }
        let c = self.config;
        let controller = &self.controller;
        controller.set_flags(&c);
        // Full screen, the modal has no look of its own: the app's popup
        // and backdrop show over what is behind. Sheets are the system's.
        let sheet = c.presentation != ModalPresentation::FullScreen;
        controller.setModalPresentationStyle(match c.presentation {
            ModalPresentation::FullScreen => UIModalPresentationStyle::OverFullScreen,
            ModalPresentation::PageSheet => UIModalPresentationStyle::PageSheet,
            ModalPresentation::FormSheet => UIModalPresentationStyle::FormSheet,
        });
        controller.setModalTransitionStyle(match c.animation {
            ModalAnimation::Fade => UIModalTransitionStyle::CrossDissolve,
            _ => UIModalTransitionStyle::CoverVertical,
        });
        if let Some(view) = controller.view() {
            let color = if sheet {
                UIColor::systemBackgroundColor()
            } else {
                UIColor::clearColor()
            };
            view.setBackgroundColor(Some(&color));
        }
        if let Some(p) = controller.presentationController() {
            // SAFETY: the controller is its own delegate and outlives the
            // presentation.
            unsafe { p.setDelegate(Some(ProtocolObject::from_ref(&**controller))) };
        }
        presenter.presentViewController_animated_completion(
            controller,
            c.animation != ModalAnimation::None,
            None,
        );
        self.presented = true;
    }

    /// Applies `config` (the close flags also while presented).
    pub(crate) fn configure(&mut self, config: ModalConfig) {
        self.controller.set_flags(&config);
        self.config = config;
    }

    /// Dismisses the modal; `done` runs when it is gone.
    pub(crate) fn dismiss(&mut self, done: impl Fn() + 'static) {
        if !self.presented {
            return;
        }
        self.presented = false;
        // Closed by the user already (and reported).
        if self.controller.ivars().closed.replace(false) {
            return;
        }
        let block = RcBlock::new(done);
        let animated = self.config.animation != ModalAnimation::None;
        // Through the presenter: a modal that presents another one (a sheet
        // over it) would otherwise only dismiss that one, and stay on
        // screen with its content already gone (closing a sheet and its
        // modal together). The presenter dismisses this modal and every
        // controller above it.
        match self.controller.presentingViewController() {
            Some(presenter) => {
                presenter.dismissViewControllerAnimated_completion(animated, Some(&block))
            }
            None => self
                .controller
                .dismissViewControllerAnimated_completion(animated, Some(&block)),
        }
    }
}

/// The view controller whose view contains `view`.
pub(crate) fn controller_of(view: &UIView) -> Option<Retained<UIViewController>> {
    let mut responder = view.nextResponder();
    while let Some(r) = responder {
        if let Some(controller) = r.downcast_ref::<UIViewController>() {
            return Some(controller.retain());
        }
        responder = r.nextResponder();
    }
    None
}

#[cfg(feature = "host-tests")]
pub(crate) mod tests {
    use super::*;
    use objc2::rc::Retained;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    fn mtm() -> objc2_foundation::MainThreadMarker {
        objc2_foundation::MainThreadMarker::new()
            .expect("the iOS host test runner runs on the main thread")
    }

    pub(crate) fn tests() -> Vec<(&'static str, fn())> {
        vec![
            (
                "navigation::a_screen_in_a_window_reports_insets",
                a_screen_in_a_window_reports_insets as fn(),
            ),
            (
                "navigation::the_insets_payload_is_points",
                the_insets_payload_is_points as fn(),
            ),
        ]
    }

    fn a_screen_in_a_window_reports_insets() {
        use objc2_foundation::NSArray;
        use objc2_ui_kit::{UIViewController, UIWindow};

        let mtm = mtm();
        let window = UIWindow::initWithFrame(
            UIWindow::alloc(mtm),
            CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)),
        );

        let root = UIViewController::new(mtm);
        let nav = NavController::new(mtm, 1);
        let screen = ScreenController::new(mtm, 2, None);

        nav.setViewControllers_animated(
            &NSArray::from_retained_slice(&[Retained::into_super(screen.clone())]),
            false,
        );

        root.view().unwrap().addSubview(&*nav.view().unwrap());
        window.setRootViewController(Some(&root));
        window.makeKeyAndVisible();
        nav.view().unwrap().layoutIfNeeded();

        screen.report_insets();

        let insets = screen.reported_insets();
        assert!(insets.is_some(), "screen should report insets after layout");

        let (top, right, bottom, left) = insets.unwrap();
        assert!(
            top.is_finite() && top >= 0.0,
            "top inset should be finite and non-negative"
        );
        assert!(
            right.is_finite() && right >= 0.0,
            "right inset should be finite and non-negative"
        );
        assert!(
            bottom.is_finite() && bottom >= 0.0,
            "bottom inset should be finite and non-negative"
        );
        assert!(
            left.is_finite() && left >= 0.0,
            "left inset should be finite and non-negative"
        );

        let safe_top = screen.view().unwrap().safeAreaInsets().top as f32;
        assert!(
            top <= safe_top,
            "reported top inset should not exceed safe area top"
        );
    }

    fn the_insets_payload_is_points() {
        use nlayer_core::geometry::px;

        let edges = EdgeInsets {
            top: px(10.0),
            right: px(20.0),
            bottom: px(30.0),
            left: px(40.0),
        };

        let payload = insets_payload(edges);

        if let EventPayload::Insets {
            top,
            right,
            bottom,
            left,
        } = payload
        {
            assert_eq!(top, 10.0);
            assert_eq!(right, 20.0);
            assert_eq!(bottom, 30.0);
            assert_eq!(left, 40.0);
        } else {
            panic!("expected Insets payload");
        }
    }
}
