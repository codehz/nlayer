//! The process's `UIApplication` and the one window it owns. The window's
//! root controller hosts the surface, and the screens' navigation controllers
//! are its children.

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSString;
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIColor, UIResponder, UIScene, UISceneConfiguration,
    UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIViewController, UIWindow,
    UIWindowScene, UIWindowSceneDelegate, UIWindowSceneSessionRoleApplication,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::host;

struct Launch {
    bundle: String,
    assets: PathBuf,
}

static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);

struct AppState {
    _window: Retained<UIWindow>,
    _session: host::Session,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl UIApplicationDelegate for AppDelegate {
        #[unsafe(method_id(application:configurationForConnectingSceneSession:options:))]
        fn configuration_for_scene(
            &self,
            _application: &UIApplication,
            session: &UISceneSession,
            _options: &UISceneConnectionOptions,
        ) -> Retained<UISceneConfiguration> {
            let config = UISceneConfiguration::initWithName_sessionRole(
                UISceneConfiguration::alloc(self.mtm()),
                None,
                &session.role(),
            );
            // SAFETY: `SceneDelegate` implements `UIWindowSceneDelegate`.
            unsafe { config.setDelegateClass(Some(SceneDelegate::class())) };
            config
        }
    }
);

define_class!(
    #[unsafe(super(UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct SceneDelegate;

    unsafe impl NSObjectProtocol for SceneDelegate {}

    unsafe impl UISceneDelegate for SceneDelegate {
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn will_connect(
            &self,
            scene: &UIScene,
            session: &UISceneSession,
            _options: &UISceneConnectionOptions,
        ) {
            // SAFETY: reading an immutable UIKit constant.
            if *session.role() != *unsafe { UIWindowSceneSessionRoleApplication } {
                return;
            }
            let Some(scene) = scene.downcast_ref::<UIWindowScene>() else {
                return;
            };
            connect(self.mtm(), scene);
        }
    }

    unsafe impl UIWindowSceneDelegate for SceneDelegate {}
);

define_class!(
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct RootController;

    unsafe impl NSObjectProtocol for RootController {}
);

impl RootController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: UIViewController's designated initializer.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn connect(mtm: MainThreadMarker, scene: &UIWindowScene) {
    let launch = LAUNCH.lock().expect("launch lock").take();
    let (bundle, assets) = match launch {
        Some(launch) => (launch.bundle, launch.assets),
        None => (String::new(), PathBuf::new()),
    };
    let window = UIWindow::initWithWindowScene(UIWindow::alloc(mtm), scene);
    let controller = RootController::new(mtm);
    let root = controller.view().expect("view controller has a view");
    // The root follows the system's appearance (black in dark mode).
    root.setBackgroundColor(Some(&UIColor::systemBackgroundColor()));
    window.setRootViewController(Some(&controller));
    window.makeKeyAndVisible();

    let session = host::start(mtm, &root, &bundle, assets);
    APP.with(|app| {
        *app.borrow_mut() = Some(AppState {
            _window: window,
            _session: session,
        });
    });
}

/// Runs `bundle` full-window. `assets` is the directory of bundled images
/// (`asset:/` paths). Never returns.
pub(crate) fn run_app(bundle: String, assets: PathBuf) -> ! {
    let mtm = MainThreadMarker::new().expect("run_app must be called on the main thread");
    *LAUNCH.lock().expect("launch lock") = Some(Launch { bundle, assets });
    let delegate = NSString::from_str(
        AppDelegate::class()
            .name()
            .to_str()
            .expect("class name is UTF-8"),
    );
    UIApplication::main(None, Some(&delegate), mtm)
}

#[cfg(feature = "host-tests")]
pub(crate) mod tests {
    use super::*;
    use nlayer_core::{MountBatch, MountItem, ROOT_ID, ViewKind};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    pub(crate) fn tests() -> Vec<(&'static str, fn())> {
        vec![(
            "app::a_stack_is_a_child_of_the_window_root",
            a_stack_is_a_child_of_the_window_root as fn(),
        )]
    }

    /// A stack's `UINavigationController` is a child of the window's root
    /// controller: appearance callbacks, safe areas and rotation reach it.
    fn a_stack_is_a_child_of_the_window_root() {
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let window = UIWindow::initWithFrame(
            UIWindow::alloc(mtm),
            CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)),
        );
        let controller = RootController::new(mtm);
        let root = controller.view().expect("view controller has a view");
        window.setRootViewController(Some(&controller));
        window.makeKeyAndVisible();

        // An empty bundle still mounts the root view, as the app's does.
        let session = host::start(mtm, &root, "", PathBuf::new());
        let mounter = host::mounter().expect("mounter");
        mounter.borrow_mut().apply(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 2,
                    kind: ViewKind::Stack,
                },
                MountItem::Insert {
                    parent: ROOT_ID,
                    child: 2,
                    before: None,
                },
                MountItem::Create {
                    id: 3,
                    kind: ViewKind::Screen,
                },
                MountItem::Insert {
                    parent: 2,
                    child: 3,
                    before: None,
                },
            ],
        });

        {
            let mounter = mounter.borrow();
            let stack = mounter.stack(2).expect("stack");
            let parent = stack
                .nav
                .parentViewController()
                .expect("a parent controller");
            assert!(
                std::ptr::eq::<UIViewController>(&*parent, &**controller),
                "the navigation controller is a child of the window root"
            );
        }
        drop(session);
        drop(window);
    }
}
