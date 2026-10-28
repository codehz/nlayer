//! The process's `UIApplication` and the one window it owns.

use std::cell::RefCell;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSString;
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIColor, UIResponder, UIScene, UISceneConfiguration,
    UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIViewController, UIWindow,
    UIWindowScene, UIWindowSceneDelegate, UIWindowSceneSessionRoleApplication,
};

use crate::host;

static LAUNCH: Mutex<Option<String>> = Mutex::new(None);

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
    let bundle = LAUNCH
        .lock()
        .expect("launch lock")
        .take()
        .unwrap_or_default();
    let window = UIWindow::initWithWindowScene(UIWindow::alloc(mtm), scene);
    let controller = RootController::new(mtm);
    let root = controller.view().expect("view controller has a view");
    // A window is black until something paints. Text defaults to black, so
    // the root is white, as React Native's is.
    root.setBackgroundColor(Some(&UIColor::whiteColor()));
    window.setRootViewController(Some(&controller));
    window.makeKeyAndVisible();
    let session = host::start(mtm, &root, &bundle);
    APP.with(|app| {
        *app.borrow_mut() = Some(AppState {
            _window: window,
            _session: session,
        });
    });
}

/// Runs `bundle` full-window. Never returns.
pub(crate) fn run_app(bundle: String) -> ! {
    let mtm = MainThreadMarker::new().expect("run_app must be called on the main thread");
    *LAUNCH.lock().expect("launch lock") = Some(bundle);
    let delegate = NSString::from_str(
        AppDelegate::class()
            .name()
            .to_str()
            .expect("class name is UTF-8"),
    );
    UIApplication::main(None, Some(&delegate), mtm)
}
