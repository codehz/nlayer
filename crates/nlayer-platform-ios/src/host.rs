//! One running surface: the engine thread and the main-thread mounter.
//! The software keyboard's overlap is sent as `env(keyboard-inset-height)`.
//! An explicit pin height replaces it; v1 does not animate with the keyboard.

use crate::keyboard::overlap;
use crate::mount::Mounter;
use crate::text::CoreTextEngine;
use block2::RcBlock;
use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Points, Size};
use nlayer_engine::{Engine, EngineConfig, EngineError, MountSink};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue, NSValue};
use objc2_ui_kit::{
    NSValueUIGeometryExtensions, UIColor, UIKeyboardFrameEndUserInfoKey,
    UIKeyboardWillChangeFrameNotification, UIKeyboardWillHideNotification, UILabel, UIResponder,
    UITraitEnvironment, UIUserInterfaceStyle, UIView, UIViewAutoresizing,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct RootIvars {
    mounter: RefCell<std::rc::Weak<RefCell<Mounter>>>,
    observers: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

define_class!(
    /// Fills the window and reports its size, safe area and keyboard overlap.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RootIvars]
    struct RootView;

    impl RootView {
        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            // `apply` still holds the mounter when UIKit lays this view
            // out. Borrowing it again aborts. The pass below runs once
            // that batch has returned.
            if self.defer_layout_if_busy() {
                return;
            }
            if let Some((width, height)) = self.pinned() {
                let frame = self.frame();
                let wanted_w = width as CGFloat;
                let wanted_h = height as CGFloat;
                if (frame.size.width - wanted_w).abs() > 0.5
                    || (frame.size.height - wanted_h).abs() > 0.5
                    || frame.origin.x != 0.0
                    || frame.origin.y != 0.0
                {
                    self.setFrame(CGRect::new(
                        CGPoint::new(0.0, 0.0),
                        CGSize::new(wanted_w, wanted_h),
                    ));
                    return;
                }
            }
            // SAFETY: the superclass lays the subviews out.
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };
            self.publish();
        }

        /// Runs after the mounter's borrow ends. `performSelector:afterDelay:`
        /// passes one object; it is unused.
        #[unsafe(method(nlayerLayoutAgain:))]
        fn layout_again(&self, _object: Option<&AnyObject>) {
            self.setNeedsLayout();
        }

        #[unsafe(method(safeAreaInsetsDidChange))]
        fn safe_area_insets_did_change(&self) {
            // SAFETY: the superclass implementation.
            let _: () = unsafe { msg_send![super(self), safeAreaInsetsDidChange] };
            self.publish();
        }

        #[unsafe(method(traitCollectionDidChange:))]
        fn trait_collection_did_change(
            &self,
            previous: Option<&objc2_ui_kit::UITraitCollection>,
        ) {
            // SAFETY: the superclass implementation.
            let _: () = unsafe { msg_send![super(self), traitCollectionDidChange: previous] };
            let dark = unsafe {
                self.traitCollection().userInterfaceStyle() == UIUserInterfaceStyle::Dark
            };
            if let Some(mounter) = self.ivars().mounter.borrow().upgrade()
                && let Ok(mut mounter) = mounter.try_borrow_mut()
            {
                mounter.set_color_scheme(dark);
            }
        }
    }

    unsafe impl NSObjectProtocol for RootView {}
);

impl RootView {
    fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RootIvars {
            mounter: RefCell::new(std::rc::Weak::new()),
            observers: RefCell::new(Vec::new()),
        });
        // SAFETY: UIView's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setAutoresizingMask(
            UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
        );
        this
    }

    fn bind(&self, mounter: std::rc::Weak<RefCell<Mounter>>) {
        *self.ivars().mounter.borrow_mut() = mounter;
    }

    /// Another layout after the mounter's current borrow ends.
    fn defer_layout_if_busy(&self) -> bool {
        let busy = self
            .ivars()
            .mounter
            .borrow()
            .upgrade()
            .is_some_and(|mounter| mounter.try_borrow().is_err());
        if busy {
            // The run loop retains `self` until the next pass, after
            // `apply` drops its borrow. A queue block cannot: the view is
            // main-thread-only and not `Send`.
            // SAFETY: `nlayerLayoutAgain:` is this class's method. Delay 0
            // is the next pass of this thread's run loop.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: objc2::sel!(nlayerLayoutAgain:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 0.0_f64
                ];
            }
        }
        busy
    }

    fn pinned(&self) -> Option<(f32, f32)> {
        let mounter = self.ivars().mounter.borrow().upgrade()?;
        mounter.try_borrow().ok()?.pin_size()
    }

    fn publish(&self) {
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        let Ok(mut mounter) = mounter.try_borrow_mut() else {
            self.defer_layout_if_busy();
            return;
        };
        let bounds = self.bounds();
        mounter.publish(bounds.size, self.safeAreaInsets());
    }

    /// Tracks the software keyboard. An explicit pin height skips this. The
    /// end frame is applied at once.
    fn observe_keyboard(&self) {
        let center = NSNotificationCenter::defaultCenter();
        let weak = Weak::new(self);
        let block = RcBlock::new(move |note: NonNull<NSNotification>| {
            let Some(view) = weak.load() else { return };
            // SAFETY: a valid notification for the duration of the call.
            view.keyboard_changed(unsafe { note.as_ref() });
        });
        // SAFETY: UIKit notification names; the block runs on the main queue.
        let names = unsafe {
            [
                UIKeyboardWillChangeFrameNotification,
                UIKeyboardWillHideNotification,
            ]
        };
        let mut observers = self.ivars().observers.borrow_mut();
        for name in names {
            // SAFETY: the observer token is removed in [`Self::stop_observing`].
            observers.push(unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            });
        }
    }

    fn stop_observing(&self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in self.ivars().observers.borrow_mut().drain(..) {
            // SAFETY: a token returned by this center.
            unsafe { center.removeObserver(observer.as_ref()) };
        }
    }

    fn keyboard_changed(&self, note: &NSNotification) {
        let Some(info) = note.userInfo() else { return };
        // SAFETY: UIKit documents this key as an NSValue of a CGRect in
        // screen coordinates.
        let frame = unsafe {
            info.objectForKey(UIKeyboardFrameEndUserInfoKey)
                .and_then(|value| value.downcast::<NSValue>().ok())
                .map(|value| value.CGRectValue())
        };
        let hiding = note.name().isEqualToString(
            // SAFETY: an immutable UIKit constant.
            unsafe { UIKeyboardWillHideNotification },
        );
        let local = match (frame, self.window().and_then(|window| window.windowScene())) {
            (Some(frame), Some(scene)) if !hiding => {
                let space = scene.screen().coordinateSpace();
                // SAFETY: `convertRect:fromCoordinateSpace:` of UICoordinateSpace.
                let frame: CGRect =
                    unsafe { msg_send![self, convertRect: frame, fromCoordinateSpace: &*space] };
                Some((
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height,
                ))
            }
            _ => None,
        };
        let bounds = self.bounds().size;
        let height = overlap(bounds.width, bounds.height, local, hiding);
        // `endEditing:` posts this while a dismiss still holds the mounter.
        // Run it now, or on the next main-queue turn if that borrow is live.
        crate::mount::with_mounter(move |mounter| mounter.set_keyboard(height));
    }
}

struct Queue {
    batches: Mutex<Vec<(u64, nlayer_core::MountBatch)>>,
    /// A drain is already queued on the main thread.
    scheduled: AtomicBool,
    /// [`recycle`] bumps this. A batch queued by the engine being dropped
    /// carries the old value and is not applied.
    epoch: AtomicU64,
}

impl MountSink for Queue {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        let epoch = self.epoch.load(Ordering::Acquire);
        let schedule = {
            let mut batches = self.batches.lock().expect("mount queue");
            batches.push((epoch, batch));
            !self.scheduled.swap(true, Ordering::AcqRel)
        };
        if !schedule {
            return;
        }
        DispatchQueue::main().exec_async(|| {
            let Some(queue) = queue() else { return };
            let current = queue.epoch.load(Ordering::Acquire);
            let batches = {
                let mut batches = queue.batches.lock().expect("mount queue");
                queue.scheduled.store(false, Ordering::Release);
                std::mem::take(&mut *batches)
            };
            let Some(mounter) = mounter() else { return };
            let mut mounter = mounter.borrow_mut();
            for (epoch, batch) in batches {
                if epoch == current {
                    mounter.apply(batch);
                }
            }
            raise_overlay();
        });
    }

    fn hot_update_failed(&self) {
        DispatchQueue::main().exec_async(|| {
            eprintln!("nlayer: reloading after an update that could not be applied in place");
            hide_dev_overlay();
            crate::dev::note_failure();
        });
    }

    fn failed(&self, error: EngineError) {
        let message = error.to_string();
        DispatchQueue::main().exec_async(move || show_error(&message));
    }

    fn request_frame(&self) -> bool {
        DispatchQueue::main().exec_async(|| {
            if let Some(mounter) = mounter() {
                mounter.borrow_mut().request_frame();
            }
        });
        true
    }
}

thread_local! {
    static MOUNTER: RefCell<Option<Rc<RefCell<Mounter>>>> = const { RefCell::new(None) };
    static QUEUE: RefCell<Option<Arc<Queue>>> = const { RefCell::new(None) };
    static ERROR: RefCell<Option<Retained<UIView>>> = const { RefCell::new(None) };
    static ERROR_TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<Weak<RootView>>> = const { RefCell::new(None) };
    static HOLDER: RefCell<Option<Weak<UIView>>> = const { RefCell::new(None) };
    static PREPARED: RefCell<Option<Prepared>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Prepared {
    scheme: nlayer_core::ColorScheme,
    pin: Option<crate::pin::Pin>,
}

pub(crate) fn mounter() -> Option<Rc<RefCell<Mounter>>> {
    MOUNTER.with(|slot| slot.borrow().clone())
}

fn queue() -> Option<Arc<Queue>> {
    QUEUE.with(|slot| slot.borrow().clone())
}

fn show_error(message: &str) {
    show_dev_overlay(message);
}

/// Covers the app with `message`. Not a UI component: the host draws it and
/// [`hide_dev_overlay`] removes it. A later mount cannot bury it.
pub(crate) fn show_dev_overlay(message: &str) {
    hide_dev_overlay();
    ERROR_TEXT.with(|slot| *slot.borrow_mut() = Some(message.to_owned()));
    eprintln!("nlayer: {message}");
    let Some(mounter) = mounter() else { return };
    let mounter = mounter.borrow();
    let Some(root) = mounter_root(&mounter) else {
        return;
    };
    let mtm = MainThreadMarker::new().expect("main thread");
    let overlay = UIView::initWithFrame(UIView::alloc(mtm), root.bounds());
    overlay.setAutoresizingMask(
        UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
    );
    overlay.setBackgroundColor(Some(&UIColor::whiteColor()));
    let label = UILabel::initWithFrame(UILabel::alloc(mtm), overlay.bounds());
    label.setAutoresizingMask(
        UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
    );
    label.setText(Some(&objc2_foundation::NSString::from_str(message)));
    // SAFETY: a plain setter of a valid color.
    unsafe { label.setTextColor(Some(&UIColor::blackColor())) };
    label.setNumberOfLines(0);
    overlay.addSubview(&label);
    root.addSubview(&overlay);
    root.bringSubviewToFront(&overlay);
    ERROR.with(|slot| *slot.borrow_mut() = Some(overlay));
}

/// Removes the dev overlay, if one is showing.
pub(crate) fn hide_dev_overlay() {
    if let Some(overlay) = ERROR.with(|slot| slot.borrow_mut().take()) {
        overlay.removeFromSuperview();
    }
    ERROR_TEXT.with(|slot| *slot.borrow_mut() = None);
}

fn raise_overlay() {
    ERROR.with(|slot| {
        let slot = slot.borrow();
        let Some(overlay) = slot.as_ref() else {
            return;
        };
        if let Some(root) = overlay.superview() {
            root.bringSubviewToFront(overlay);
        }
    });
}

fn mounter_root(mounter: &Mounter) -> Option<Retained<UIView>> {
    mounter.view_of(nlayer_core::ROOT_ID)
}

#[cfg_attr(not(feature = "host-tests"), allow(dead_code))]
pub(crate) fn boot_error() -> Option<String> {
    ERROR_TEXT.with(|slot| slot.borrow().clone())
}

/// A running app. Dropping it stops the engine.
pub(crate) struct Session {
    _root: Retained<RootView>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self._root.stop_observing();
        ENGINE.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

/// Starts the engine on `bundle` inside `container` (the window's root view).
/// `assets` is the directory bundled `asset:/` images are read from. A pin
/// file there replaces the window size and safe area for `nlayer test`.
pub(crate) fn start(
    mtm: MainThreadMarker,
    container: &UIView,
    bundle: &str,
    assets: PathBuf,
) -> Session {
    let pin = crate::pin::load(&assets);
    let frame = pin.as_ref().map_or_else(
        || container.bounds(),
        |pin| {
            CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
            )
        },
    );
    let root = RootView::new(mtm, frame);
    if pin.is_some() {
        root.setAutoresizingMask(UIViewAutoresizing::empty());
    }
    // A pinned surface bigger than the window (a test's viewport) is shown
    // as one, scaled down: shrinking it would change the layout the test
    // asked for, and a part left outside the window takes no touches.
    let scaled = pin.as_ref().and_then(|pin| {
        let size = container.bounds().size;
        let scale = (size.width as f32 / pin.width)
            .min(size.height as f32 / pin.height)
            .min(1.0);
        (scale < 1.0).then_some(scale)
    });
    if let (Some(pin), Some(scale)) = (&pin, scaled) {
        let size = container.bounds().size;
        let holder = UIView::new(mtm);
        holder.setBounds(CGRect::new(
            CGPoint::ZERO,
            CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
        ));
        holder.setCenter(CGPoint::new(size.width / 2.0, size.height / 2.0));
        holder.setTransform(CGAffineTransform {
            a: scale as CGFloat,
            b: 0.0,
            c: 0.0,
            d: scale as CGFloat,
            tx: 0.0,
            ty: 0.0,
        });
        holder.setClipsToBounds(true);
        holder.addSubview(&root);
        container.addSubview(&holder);
        HOLDER.with(|slot| *slot.borrow_mut() = Some(Weak::new(&holder)));
    } else {
        container.addSubview(&root);
    }
    let mounter = Mounter::new(mtm, root.retain().into_super(), assets.clone());
    if let Some(pin) = pin.clone() {
        mounter.borrow_mut().set_pin(pin);
    }
    root.bind(Rc::downgrade(&mounter));
    if pin.as_ref().is_none_or(|pin| pin.keyboard.is_none()) {
        root.observe_keyboard();
    }
    MOUNTER.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&mounter)));
    QUEUE.with(|slot| {
        *slot.borrow_mut() = Some(Arc::new(Queue {
            batches: Mutex::new(Vec::new()),
            scheduled: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
        }));
    });
    let scheme = pin
        .as_ref()
        .and_then(|pin| pin.color_scheme.as_deref())
        .map(|name| nlayer_core::ColorScheme::from_dark(name == "dark"))
        .unwrap_or_else(|| {
            let style = unsafe { container.traitCollection().userInterfaceStyle() };
            nlayer_core::ColorScheme::from_dark(style == UIUserInterfaceStyle::Dark)
        });
    PREPARED.with(|slot| {
        *slot.borrow_mut() = Some(Prepared {
            scheme,
            pin: pin.clone(),
        });
    });
    ROOT.with(|slot| *slot.borrow_mut() = Some(Weak::new(&root)));

    let dev = crate::pin::load_dev(&assets);
    let control = if bundle.is_empty() {
        None
    } else {
        match launch(bundle) {
            Ok(control) => Some(control),
            Err(error) => {
                // A test pin loads its script from the socket. No overlay.
                if pin.is_none() {
                    show_dev_overlay(&error.to_string());
                } else {
                    eprintln!("nlayer: {error}");
                }
                None
            }
        }
    };
    // An empty `app.js` with a dev pin waits for the first bundle. A test
    // pin does too, and does not cover the app. The missing-file error is
    // only for a launch that has nowhere to read one.
    if bundle.is_empty() && pin.is_none() && dev.as_ref().is_none_or(|dev| dev.test) {
        show_dev_overlay("no app.js next to the runner");
    } else if let Some(dev) = dev.filter(|dev| !dev.test) {
        let script = if control.is_some() {
            bundle.to_owned()
        } else {
            String::new()
        };
        crate::dev::start(dev, script, control.clone());
    }
    if let Some(pin) = pin {
        let script = if control.is_some() {
            bundle.to_owned()
        } else {
            String::new()
        };
        crate::dev::start_test(pin, script, control);
    }
    Session { _root: root }
}

/// Evaluates `bundle` on the surface prepared by [`start`]. Main thread.
fn launch(bundle: &str) -> Result<nlayer_engine::EngineControl, EngineError> {
    let prepared = PREPARED
        .with(|slot| slot.borrow().clone())
        .expect("surface");
    let mounter = mounter().expect("mounter");
    let queue = queue().expect("queue");
    let root = {
        let mounter = mounter.borrow();
        mounter_root(&mounter).expect("root")
    };
    let bounds = root.bounds().size;
    let viewport = Size::new(Points(bounds.width as f32), Points(bounds.height as f32));
    let mut config = EngineConfig::new(bundle, viewport)
        .with_os(nlayer_engine::Os::Ios)
        .with_color_scheme(prepared.scheme);
    // The app's storage: a file in Application Support of its container.
    if let Some(dir) = nlayer_engine::data_dir() {
        config = config.with_data_dir(dir.join("nlayer"));
    }
    if let Some(pin) = &prepared.pin {
        config = config.with_insets(
            nlayer_core::geometry::Edges {
                top: Points(pin.safe_area.top),
                right: Points(pin.safe_area.right),
                bottom: Points(pin.safe_area.bottom),
                left: Points(pin.safe_area.left),
            },
            Points(pin.keyboard.unwrap_or(0.0)),
        );
    }
    let engine = Engine::start(config, CoreTextEngine::default, queue)?;
    mounter.borrow_mut().set_control(engine.control());
    republish();
    let control = engine.control();
    ENGINE.with(|slot| *slot.borrow_mut() = Some(engine));
    Ok(control)
}

/// Starts the engine on `bundle` if it is not running. Main thread.
/// `None` leaves the overlay up; the dev client can try the next bundle.
pub(crate) fn boot_engine(bundle: &str) -> Option<nlayer_engine::EngineControl> {
    boot_engine_showing(bundle, true)
}

/// [`boot_engine`] without the dev overlay. A test session uses this.
pub(crate) fn boot_engine_quiet(bundle: &str) -> Option<nlayer_engine::EngineControl> {
    boot_engine_showing(bundle, false)
}

/// Drops the engine and the native tree, and applies `next` before the
/// following bundle starts a new QuickJS runtime. Main thread. The window
/// stays.
pub(crate) fn recycle(next: nlayer_engine::protocol::Recycle) {
    let pin = PREPARED.with(|slot| {
        let mut slot = slot.borrow_mut();
        let prepared = slot.as_mut()?;
        let mut pin = prepared.pin.clone().unwrap_or_else(|| crate::pin::Pin {
            width: next.width,
            height: next.height,
            keyboard: Some(next.keyboard),
            safe_area: crate::pin::Area::default(),
            color_scheme: next.color_scheme.clone(),
            host: String::new(),
            port: 0,
            test: true,
        });
        pin.width = next.width;
        pin.height = next.height;
        pin.keyboard = Some(next.keyboard);
        pin.safe_area = crate::pin::Area {
            top: next.safe_area.top,
            right: next.safe_area.right,
            bottom: next.safe_area.bottom,
            left: next.safe_area.left,
        };
        if let Some(scheme) = next.color_scheme.clone() {
            prepared.scheme = nlayer_core::ColorScheme::from_dark(scheme == "dark");
            pin.color_scheme = Some(scheme);
        }
        prepared.pin = Some(pin.clone());
        Some(pin)
    });
    let Some(pin) = pin else {
        return;
    };
    if let Some(mounter) = mounter() {
        mounter.borrow_mut().set_pin(pin.clone());
    }
    place_pinned_root(&pin);
    // Join the engine before bumping the epoch, so a batch it submits on
    // the way out still carries the old one.
    ENGINE.with(|slot| {
        slot.borrow_mut().take();
    });
    if let Some(dir) = nlayer_engine::data_dir() {
        nlayer_engine::clear_storage(&dir.join("nlayer"));
    }
    if let Some(queue) = queue() {
        queue.epoch.fetch_add(1, Ordering::AcqRel);
    }
    if let Some(mounter) = mounter() {
        mounter.borrow_mut().clear_app();
    }
}

/// Sizes the pinned root, and the scale holder when the viewport is larger
/// than the window. A later `open` can change both.
fn place_pinned_root(pin: &crate::pin::Pin) {
    let Some(root) = ROOT.with(|slot| slot.borrow().as_ref().and_then(|weak| weak.load())) else {
        return;
    };
    let mtm = MainThreadMarker::new().expect("recycle runs on the main thread");
    root.setFrame(CGRect::new(
        CGPoint::ZERO,
        CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
    ));
    root.setAutoresizingMask(UIViewAutoresizing::empty());
    let holder = HOLDER.with(|slot| slot.borrow().as_ref().and_then(|weak| weak.load()));
    let container = holder
        .as_ref()
        .and_then(|holder| holder.superview())
        .or_else(|| root.superview());
    let Some(container) = container else {
        return;
    };
    let size = container.bounds().size;
    let scale = (size.width as f32 / pin.width.max(1.0))
        .min(size.height as f32 / pin.height.max(1.0))
        .min(1.0);
    if scale < 1.0 {
        let holder = if let Some(holder) = holder {
            holder
        } else {
            let created = UIView::new(mtm);
            root.removeFromSuperview();
            created.addSubview(&root);
            container.addSubview(&created);
            HOLDER.with(|slot| *slot.borrow_mut() = Some(Weak::new(&created)));
            created
        };
        holder.setBounds(CGRect::new(
            CGPoint::ZERO,
            CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
        ));
        holder.setCenter(CGPoint::new(size.width / 2.0, size.height / 2.0));
        holder.setTransform(CGAffineTransform {
            a: scale as CGFloat,
            b: 0.0,
            c: 0.0,
            d: scale as CGFloat,
            tx: 0.0,
            ty: 0.0,
        });
        holder.setClipsToBounds(true);
    } else if let Some(holder) = holder {
        root.removeFromSuperview();
        container.addSubview(&root);
        holder.removeFromSuperview();
        HOLDER.with(|slot| *slot.borrow_mut() = None);
    }
}

fn boot_engine_showing(bundle: &str, overlay: bool) -> Option<nlayer_engine::EngineControl> {
    if let Some(control) = ENGINE.with(|slot| slot.borrow().as_ref().map(|engine| engine.control()))
    {
        return Some(control);
    }
    match launch(bundle) {
        Ok(control) => Some(control),
        Err(error) => {
            if overlay {
                show_dev_overlay(&error.to_string());
            } else {
                eprintln!("nlayer: {error}");
            }
            None
        }
    }
}

fn republish() {
    let Some(root) = ROOT.with(|slot| slot.borrow().as_ref().and_then(|weak| weak.load())) else {
        return;
    };
    root.publish();
}

#[cfg(feature = "host-tests")]
pub(crate) mod tests {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_core_foundation::{CGRect, CGSize};

    fn assets() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nlayer-host-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temporary assets");
        dir
    }

    pub(crate) fn tests() -> Vec<(&'static str, fn())> {
        vec![(
            "host::an_empty_bundle_reports_the_missing_entry_and_keeps_a_root",
            an_empty_bundle_reports_the_missing_entry_and_keeps_a_root as fn(),
        )]
    }

    fn an_empty_bundle_reports_the_missing_entry_and_keeps_a_root() {
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let container = UIView::new(mtm);
        container.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
        let session = start(mtm, &container, "", assets());

        assert_eq!(
            boot_error().as_deref(),
            Some("no app.js next to the runner")
        );
        let mounter = mounter().expect("mounter");
        let root = mounter
            .borrow()
            .view_of(nlayer_core::ROOT_ID)
            .expect("root view");
        assert!(root.isDescendantOfView(&container));
        assert_eq!(root.bounds().size.width, 320.0);
        assert_eq!(root.bounds().size.height, 640.0);
        // The error label is on the root the app would have drawn into.
        assert!(matches!(root.subviews().count(), 1..));
        assert_eq!(mounter.borrow().scene().tree()["frame"]["width"], 320.0);
        drop(session);
    }
}
