//! One running surface: the engine thread and the main-thread mounter.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::mount::Mounter;
use crate::text::CoreTextEngine;
use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Points, Size};
use nlayer_engine::{Engine, EngineConfig, EngineError, MountSink};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_core_foundation::CGRect;
use objc2_ui_kit::{UILabel, UIResponder, UIView, UIViewAutoresizing};

struct RootIvars {
    mounter: RefCell<std::rc::Weak<RefCell<Mounter>>>,
}

define_class!(
    /// Fills the window and reports its size and safe area to the engine.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RootIvars]
    struct RootView;

    impl RootView {
        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
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
    }

    unsafe impl NSObjectProtocol for RootView {}
);

impl RootView {
    fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RootIvars {
            mounter: RefCell::new(std::rc::Weak::new()),
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

    fn publish(&self) {
        let Some(mounter) = self.ivars().mounter.borrow().upgrade() else {
            return;
        };
        let bounds = self.bounds();
        mounter
            .borrow_mut()
            .publish(bounds.size, self.safeAreaInsets());
    }
}

struct Queue(Mutex<Vec<nlayer_core::MountBatch>>);

impl MountSink for Queue {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        self.0.lock().expect("mount queue").push(batch);
        DispatchQueue::main().exec_async(|| {
            let Some(queue) = queue() else { return };
            let batches = std::mem::take(&mut *queue.0.lock().expect("mount queue"));
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
}

thread_local! {
    static MOUNTER: RefCell<Option<Rc<RefCell<Mounter>>>> = const { RefCell::new(None) };
    static QUEUE: RefCell<Option<Arc<Queue>>> = const { RefCell::new(None) };
    static ERROR: RefCell<Option<Retained<UILabel>>> = const { RefCell::new(None) };
}

fn mounter() -> Option<Rc<RefCell<Mounter>>> {
    MOUNTER.with(|slot| slot.borrow().clone())
}

fn queue() -> Option<Arc<Queue>> {
    QUEUE.with(|slot| slot.borrow().clone())
}

fn show_error(message: &str) {
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

/// A running app. Dropping it stops the engine.
pub(crate) struct Session {
    _engine: Option<Engine>,
    _root: Retained<RootView>,
}

/// Starts the engine on `bundle` inside `container` (the window's root view).
pub(crate) fn start(mtm: MainThreadMarker, container: &UIView, bundle: &str) -> Session {
    let root = RootView::new(mtm, container.bounds());
    container.addSubview(&root);
    let mounter = Mounter::new(mtm, root.retain().into_super());
    root.bind(Rc::downgrade(&mounter));
    MOUNTER.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&mounter)));
    let queue = Arc::new(Queue(Mutex::new(Vec::new())));
    QUEUE.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&queue)));

    if bundle.is_empty() {
        show_error("no app.js next to the runner");
        return Session {
            _engine: None,
            _root: root,
        };
    }

    let bounds = root.bounds().size;
    let viewport = Size::new(Points(bounds.width as f32), Points(bounds.height as f32));
    let engine = Engine::start(
        EngineConfig::new(bundle, viewport).with_os(nlayer_engine::Os::Ios),
        CoreTextEngine::default,
        queue,
    );
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
    Session {
        _engine: engine,
        _root: root,
    }
}
