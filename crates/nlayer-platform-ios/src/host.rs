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
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue, NSValue};
use objc2_ui_kit::{
    NSValueUIGeometryExtensions, UIKeyboardFrameEndUserInfoKey,
    UIKeyboardWillChangeFrameNotification, UIKeyboardWillHideNotification, UILabel, UIResponder,
    UITraitEnvironment, UIUserInterfaceStyle, UIView, UIViewAutoresizing,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
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

    fn pinned(&self) -> Option<(f32, f32)> {
        self.ivars()
            .mounter
            .borrow()
            .upgrade()
            .and_then(|mounter| mounter.borrow().pin_size())
    }

    fn publish(&self) {
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        let bounds = self.bounds();
        mounter
            .borrow_mut()
            .publish(bounds.size, self.safeAreaInsets());
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
    batches: Mutex<Vec<nlayer_core::MountBatch>>,
    /// A drain is already queued on the main thread.
    scheduled: AtomicBool,
}

impl MountSink for Queue {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        let schedule = {
            let mut batches = self.batches.lock().expect("mount queue");
            batches.push(batch);
            !self.scheduled.swap(true, Ordering::AcqRel)
        };
        if !schedule {
            return;
        }
        DispatchQueue::main().exec_async(|| {
            let Some(queue) = queue() else { return };
            let batches = {
                let mut batches = queue.batches.lock().expect("mount queue");
                queue.scheduled.store(false, Ordering::Release);
                std::mem::take(&mut *batches)
            };
            let Some(mounter) = mounter() else { return };
            let mut mounter = mounter.borrow_mut();
            for batch in batches {
                mounter.apply(batch);
            }
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
    static ERROR: RefCell<Option<Retained<UILabel>>> = const { RefCell::new(None) };
    static ERROR_TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn mounter() -> Option<Rc<RefCell<Mounter>>> {
    MOUNTER.with(|slot| slot.borrow().clone())
}

fn queue() -> Option<Arc<Queue>> {
    QUEUE.with(|slot| slot.borrow().clone())
}

fn show_error(message: &str) {
    ERROR_TEXT.with(|slot| *slot.borrow_mut() = Some(message.to_owned()));
    eprintln!("nlayer: {message}");
    let Some(mounter) = mounter() else { return };
    let mounter = mounter.borrow();
    let Some(root) = mounter_root(&mounter) else {
        return;
    };
    let mtm = MainThreadMarker::new().expect("main thread");
    let label = UILabel::initWithFrame(UILabel::alloc(mtm), root.bounds());
    label.setText(Some(&objc2_foundation::NSString::from_str(message)));
    // SAFETY: a plain setter of a valid color.
    unsafe { label.setTextColor(Some(&objc2_ui_kit::UIColor::blackColor())) };
    label.setNumberOfLines(0);
    root.addSubview(&label);
    ERROR.with(|slot| *slot.borrow_mut() = Some(label));
}

fn mounter_root(mounter: &Mounter) -> Option<Retained<UIView>> {
    mounter.view_of(nlayer_core::ROOT_ID)
}

pub(crate) fn boot_error() -> Option<String> {
    ERROR_TEXT.with(|slot| slot.borrow().clone())
}

/// A running app. Dropping it stops the engine.
pub(crate) struct Session {
    _engine: Option<Engine>,
    _root: Retained<RootView>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self._root.stop_observing();
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
    container.addSubview(&root);
    let mounter = Mounter::new(mtm, root.retain().into_super(), assets.clone());
    if let Some(pin) = pin.clone() {
        mounter.borrow_mut().set_pin(pin);
    }
    root.bind(Rc::downgrade(&mounter));
    if pin.as_ref().is_none_or(|pin| pin.keyboard.is_none()) {
        root.observe_keyboard();
    }
    MOUNTER.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&mounter)));
    let queue = Arc::new(Queue {
        batches: Mutex::new(Vec::new()),
        scheduled: AtomicBool::new(false),
    });
    QUEUE.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&queue)));

    if bundle.is_empty() {
        show_error("no app.js next to the runner");
        if let Some(pin) = pin {
            crate::driver::start(pin);
        }
        return Session {
            _engine: None,
            _root: root,
        };
    }

    let bounds = root.bounds().size;
    let viewport = Size::new(Points(bounds.width as f32), Points(bounds.height as f32));
    let scheme = pin
        .as_ref()
        .and_then(|pin| pin.color_scheme.as_deref())
        .map(|name| nlayer_core::ColorScheme::from_dark(name == "dark"))
        .unwrap_or_else(|| {
            let style = unsafe { container.traitCollection().userInterfaceStyle() };
            nlayer_core::ColorScheme::from_dark(style == UIUserInterfaceStyle::Dark)
        });
    let mut config = EngineConfig::new(bundle, viewport)
        .with_os(nlayer_engine::Os::Ios)
        .with_color_scheme(scheme);
    // The app's storage: a file in Application Support of its container.
    if let Some(dir) = nlayer_engine::data_dir() {
        config = config.with_data_dir(dir.join("nlayer"));
    }
    if let Some(pin) = &pin {
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
    let engine = Engine::start(config, CoreTextEngine::default, queue);
    let engine = match engine {
        Ok(engine) => {
            mounter.borrow_mut().set_control(engine.control());
            root.publish();
            Some(engine)
        }
        Err(err) => {
            show_error(&err.to_string());
            None
        }
    };
    if let Some(pin) = pin {
        crate::driver::start(pin);
    }
    Session {
        _engine: engine,
        _root: root,
    }
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
