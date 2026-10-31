//! The process's `UIApplication` and the one window it owns. The window root
//! hosts the surface controller, which hosts navigation.

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSString;
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIColor, UIResponder, UIScene, UISceneConfiguration,
    UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIViewAutoresizing,
    UIViewController, UIWindow, UIWindowScene, UIWindowSceneDelegate,
    UIWindowSceneSessionRoleApplication,
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

define_class!(
    #[unsafe(super(UIViewController, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct SurfaceController;

    unsafe impl NSObjectProtocol for SurfaceController {}
);

impl SurfaceController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: UIViewController's designated initializer.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

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

    // Add surface controller as a child of the root controller.
    let surface = SurfaceController::new(mtm);
    controller.addChildViewController(&surface);
    let surface_view = surface.view().expect("view controller has a view");
    surface_view.setBackgroundColor(Some(&UIColor::systemBackgroundColor()));
    surface_view.setFrame(root.bounds());
    surface_view.setAutoresizingMask(
        UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
    );
    root.addSubview(&surface_view);
    surface.didMoveToParentViewController(Some(&controller));

    let session = host::start(mtm, &surface_view, &bundle, assets);
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
