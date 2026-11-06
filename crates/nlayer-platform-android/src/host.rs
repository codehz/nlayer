//! One running surface: the engine thread and the UI-thread mounter.
//!
//! Extracted from `nsh-platform-android/src/host.rs` and cut to one surface,
//! press, input, scroll events and safe area. The dev server's client lives
//! in `dev.rs`.
#![allow(unreachable_pub)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use jni::{
    EnvUnowned,
    errors::LogErrorAndDefault,
    objects::{JByteArray, JClass, JFloatArray, JObject, JObjectArray, JString},
    refs::Global,
    sys::{jboolean, jfloat, jint, jlong},
};
use nlayer_core::geometry::{Edges, Point, Points, Size, px};
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::{Engine, EngineConfig, EngineControl, EventPayload, MountSink, Os};

use crate::encode::{self, Object};
use crate::java::{HostView, with_env};
use crate::text::{AndroidTextEngine, Density};

pub(crate) struct Host {
    pub(crate) view: Arc<Global<HostView<'static>>>,
    pub(crate) engine: Mutex<Option<Engine>>,
    pub(crate) control: Mutex<Option<EngineControl>>,
    /// `app.js` the engine is running, after a start succeeded.
    pub(crate) boot_script: Mutex<String>,
    /// Mount batches the engine has handed to the UI thread. A test waits
    /// for this to move before its first step, then joins the UI thread.
    pub(crate) commits: Arc<AtomicU64>,
    /// What a later bundle needs to start the engine, if the first start failed.
    launch: Mutex<Option<Launch>>,
    density: Density,
    size: Mutex<Size<Points>>,
    insets: Mutex<Option<(Edges<Points>, Points)>>,
}

#[derive(Clone)]
struct Launch {
    data_dir: String,
    dark: bool,
}

struct Sink {
    view: Arc<Global<HostView<'static>>>,
    commits: Arc<AtomicU64>,
}

impl MountSink for Sink {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        crate::animation::note(&batch.items);
        let encoded = encode::encode(batch);
        with_env(|env| {
            let bytes = env.byte_array_from_slice(&encoded.bytes)?;
            let objects =
                JObjectArray::<JObject>::new(env, encoded.objects.len(), &JObject::null())?;
            for (index, object) in encoded.objects.iter().enumerate() {
                match object {
                    Object::Str(text) => {
                        let text = env.new_string(text)?;
                        objects.set_element(env, index, &text)?;
                    }
                    Object::Layout(layout) => {
                        objects.set_element(env, index, layout.as_obj())?;
                    }
                }
            }
            self.view.submit(env, &bytes, &objects)?;
            Ok(())
        });
        // After `submit` returns, the batch is on the UI thread's queue: a
        // `snapshot` posted later runs after it.
        self.commits.fetch_add(1, Ordering::SeqCst);
    }

    fn failed(&self, error: nlayer_engine::EngineError) {
        report(&self.view, &error.message);
    }

    fn hot_update_failed(&self) {
        // The engine reloads the last full bundle itself; the overlay is
        // about builds, not this update.
        overlay_view(&self.view, None);
    }

    fn back_unhandled(&self) {
        with_env(|env| {
            self.view.back_unhandled(env)?;
            Ok(())
        });
    }

    fn request_frame(&self) -> bool {
        with_env(|env| {
            self.view.request_frame(env)?;
            Ok(true)
        })
    }
}

fn report(view: &Arc<Global<HostView<'static>>>, message: &str) {
    with_env(|env| {
        let message = env.new_string(message)?;
        view.report_error(env, &message)?;
        Ok(())
    });
}

/// Shows the development overlay over the app, or removes it when `message`
/// is `None`. Not part of the app tree: the view adds it as its own child,
/// outside the mounted views. Posts to the UI thread like [`report`].
pub(crate) fn overlay_view(view: &Arc<Global<HostView<'static>>>, message: Option<&str>) {
    with_env(|env| {
        let message = match message {
            Some(message) => env.new_string(message)?,
            None => JString::null(),
        };
        view.overlay(env, &message)?;
        Ok(())
    });
}

/// # Safety
///
/// `handle` is a live pointer returned by [`Java_dev_nlayer_android_Native_create`].
unsafe fn host<'a>(handle: jlong) -> &'a Arc<Host> {
    unsafe { &*(handle as *const Arc<Host>) }
}

/// # Safety
///
/// Called from Java with a live `HostView`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_create(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    view: JObject<'_>,
    density: jfloat,
) -> jlong {
    env.with_env(|env| -> jni::errors::Result<jlong> {
        let view = HostView::cast_local(env, view)?;
        let view = Arc::new(env.new_global_ref(view)?);
        let host = Arc::new(Host {
            view,
            engine: Mutex::new(None),
            control: Mutex::new(None),
            boot_script: Mutex::new(String::new()),
            commits: Arc::new(AtomicU64::new(0)),
            launch: Mutex::new(None),
            density: Density::new(density),
            size: Mutex::new(Size::new(px(0.0), px(0.0))),
            insets: Mutex::new(None),
        });
        Ok(Box::into_raw(Box::new(host)) as jlong)
    })
    .resolve::<LogErrorAndDefault>()
}

/// # Safety
///
/// `handle` is a live pointer from [`Java_dev_nlayer_android_Native_create`],
/// or 0.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_destroy(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // Dropping the engine stops its thread.
    drop(unsafe { Box::from_raw(handle as *mut Arc<Host>) });
}

/// # Safety
///
/// `handle` is live. `code` is a UTF-8 bundle. `quiet` skips the error
/// cover: a test pin loads its script from the socket instead.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_load(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    code: JByteArray<'_>,
    _assets: JString<'_>,
    data_dir: JString<'_>,
    dark: jboolean,
    quiet: jboolean,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    env.with_env(|env| -> jni::errors::Result<()> {
        let bytes = match env.convert_byte_array(&code) {
            Ok(bytes) => bytes,
            Err(err) => {
                if !quiet {
                    report(&host.view, &format!("cannot read the bundle: {err}"));
                } else {
                    log::error!("nlayer: cannot read the bundle: {err}");
                }
                return Ok(());
            }
        };
        let bundle = String::from_utf8_lossy(&bytes).into_owned();
        let dir = data_dir.try_to_string(env).unwrap_or_default();
        *host.launch.lock().unwrap_or_else(|err| err.into_inner()) = Some(Launch {
            data_dir: dir.clone(),
            dark: dark,
        });
        match start_engine(host, &bundle, &dir, dark) {
            Ok(()) => {
                *host
                    .boot_script
                    .lock()
                    .unwrap_or_else(|err| err.into_inner()) = bundle;
            }
            Err(err) => {
                if !quiet {
                    report(&host.view, &err);
                } else {
                    log::error!("nlayer: {err}");
                }
            }
        }
        Ok(())
    })
    .resolve::<LogErrorAndDefault>();
}

/// Starts the engine on `code` if it is not running. A test session calls
/// this when `app.js` never started. No overlay.
pub(crate) fn boot(host: &Host, code: &str) -> Option<EngineControl> {
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    {
        return Some(control);
    }
    let launch = host
        .launch
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()?;
    match start_engine(host, code, &launch.data_dir, launch.dark) {
        Ok(()) => host
            .control
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone(),
        Err(error) => {
            log::error!("nlayer: {error}");
            None
        }
    }
}

fn start_engine(host: &Host, bundle: &str, data_dir: &str, dark: bool) -> Result<(), String> {
    let size = *host.size.lock().unwrap_or_else(|err| err.into_inner());
    let viewport = if size.width.0 > 0.0 && size.height.0 > 0.0 {
        size
    } else {
        Size::new(px(1.0), px(1.0))
    };
    crate::animation::reset();
    let density = host.density.clone();
    let sink = Arc::new(Sink {
        view: Arc::clone(&host.view),
        commits: Arc::clone(&host.commits),
    });
    let mut config = EngineConfig::new(bundle, viewport)
        .with_os(Os::Android)
        // The bar is 44pt with no large title, not a 56dp Toolbar.
        .with_header_heights((Points(44.0), Points(44.0)))
        .with_color_scheme(nlayer_core::ColorScheme::from_dark(dark));
    if !data_dir.is_empty() {
        config = config.with_data_dir(data_dir);
    }
    // The first frame already lays out with the current insets: a recycled
    // test app asks for another safe area before its bundle starts.
    if let Some((edges, keyboard)) = *host.insets.lock().unwrap_or_else(|err| err.into_inner()) {
        config = config.with_insets(edges, keyboard);
    }
    match Engine::start(config, move || AndroidTextEngine { density }, sink) {
        Ok(engine) => {
            *host.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine.control());
            *host.engine.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine);
            apply_insets(host);
            Ok(())
        }
        Err(err) => Err(err.message),
    }
}

/// Connects the running engine to a `nlayer run` dev server.
///
/// # Safety
///
/// `handle` is live. `address` is the dev server (`127.0.0.1` after
/// `adb reverse`).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_dev(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    address: JString<'_>,
    port: jint,
    test: jboolean,
) {
    if handle == 0 || port <= 0 || port > u16::MAX as jint {
        return;
    }
    let host = Arc::clone(unsafe { host(handle) });
    let address = env
        .with_env(|env| -> jni::errors::Result<String> {
            Ok(address.try_to_string(env).unwrap_or_default())
        })
        .resolve::<LogErrorAndDefault>();
    if address.is_empty() {
        return;
    }
    crate::dev::start(host, address, port as u16, test);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_animating(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jboolean {
    if handle == 0 {
        return false;
    }
    crate::animation::animating()
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_tick<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    nanos: jlong,
) -> JByteArray<'local> {
    if handle == 0 {
        return JByteArray::null();
    }
    let batch = crate::animation::frame(nanos as f64 / 1_000_000_000.0);
    let finished = crate::animation::take_finished();
    if !finished.is_empty()
        && let Some(control) = unsafe { host(handle) }
            .control
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
    {
        control.animations_finished(finished);
    }
    let encoded = encode::encode(batch);
    env.with_env(|env| -> jni::errors::Result<JByteArray<'local>> {
        env.byte_array_from_slice(&encoded.bytes)
    })
    .resolve::<LogErrorAndDefault>()
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_resize(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    width: jfloat,
    height: jfloat,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    let size = Size::new(px(width), px(height));
    *host.size.lock().unwrap_or_else(|err| err.into_inner()) = size;
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.resize(size);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_colorScheme(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    dark: jboolean,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.set_color_scheme(nlayer_core::ColorScheme::from_dark(dark));
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_back(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.back();
    }
}

/// # Safety
///
/// `handle` is live. Insets are points.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_insets(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    top: jfloat,
    right: jfloat,
    bottom: jfloat,
    left: jfloat,
    keyboard: jfloat,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    *host.insets.lock().unwrap_or_else(|err| err.into_inner()) = Some((
        Edges {
            top: Points(top),
            right: Points(right),
            bottom: Points(bottom),
            left: Points(left),
        },
        Points(keyboard),
    ));
    apply_insets(host);
}

fn apply_insets(host: &Host) {
    let Some((edges, keyboard)) = *host.insets.lock().unwrap_or_else(|err| err.into_inner()) else {
        return;
    };
    let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    else {
        return;
    };
    control.set_safe_area(edges);
    control.set_keyboard_inset(keyboard);
}

/// Drops the engine and the native tree, and stores `next` for the bundle
/// that starts the next QuickJS runtime. The activity stays.
pub(crate) fn recycle(host: &Host, next: nlayer_engine::protocol::Recycle) -> Result<(), String> {
    // Join the engine thread before the UI thread clears views, so its last
    // batch is already queued and runs before that clear.
    drop(
        host.engine
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take(),
    );
    *host.control.lock().unwrap_or_else(|err| err.into_inner()) = None;
    *host
        .boot_script
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = String::new();
    *host.size.lock().unwrap_or_else(|err| err.into_inner()) =
        Size::new(px(next.width), px(next.height));
    *host.insets.lock().unwrap_or_else(|err| err.into_inner()) = Some((
        Edges {
            top: Points(next.safe_area.top),
            right: Points(next.safe_area.right),
            bottom: Points(next.safe_area.bottom),
            left: Points(next.safe_area.left),
        },
        Points(next.keyboard),
    ));
    if let Some(scheme) = next.color_scheme.as_deref()
        && let Some(launch) = host
            .launch
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_mut()
    {
        launch.dark = scheme == "dark";
    }
    crate::animation::reset();
    recycle_view(host, &next)
}

fn recycle_view(host: &Host, next: &nlayer_engine::protocol::Recycle) -> Result<(), String> {
    crate::java::try_with_env(|env| {
        let scheme = match &next.color_scheme {
            Some(scheme) => env.new_string(scheme)?,
            None => JString::null(),
        };
        host.view.recycle(
            env,
            next.width,
            next.height,
            next.keyboard,
            next.safe_area.top,
            next.safe_area.right,
            next.safe_area.bottom,
            next.safe_area.left,
            &scheme,
        )?;
        Ok(())
    })
}

/// # Safety
///
/// `handle` is live. `address` is the dev server (`127.0.0.1` after
/// `adb reverse`). The host connects `ws://address:port/ws`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_serve(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    address: JString<'_>,
    port: jint,
) {
    if handle == 0 || port == 0 {
        return;
    }
    let host = Arc::clone(unsafe { host(handle) });
    let address: String = env
        .with_env(|env| -> jni::errors::Result<String> {
            Ok(address.try_to_string(env).unwrap_or_default())
        })
        .resolve::<LogErrorAndDefault>();
    if address.is_empty() {
        return;
    }
    crate::dev::start_test(host, address, port as u16);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_press(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    dispatch(handle, id, EventKind::Press, EventPayload::None);
}

/// Interaction states of a view the core tracks (`active:`, `hover:`, `focus:`).
///
/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_states(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    states: jint,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.set_states(id as NodeId, states as u16);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_longPress(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    dispatch(handle, id, EventKind::LongPress, EventPayload::None);
}

/// # Safety
///
/// `handle` is live. `kind` is an [`EventKind`] discriminant. Lengths are
/// points: content offset, content size, then the view's visible size.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_scroll(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    kind: jint,
    x: jfloat,
    y: jfloat,
    content_w: jfloat,
    content_h: jfloat,
    width: jfloat,
    height: jfloat,
) {
    let Ok(raw) = u8::try_from(kind) else {
        return;
    };
    let Some(event) = EventKind::from_raw(raw) else {
        return;
    };
    // Update core's scroll position for list windowing, sticky headers, and viewport=parent
    // (same as web does in nlayer-platform-web/src/page.rs)
    if event == EventKind::Scroll && handle != 0 {
        let host = unsafe { host(handle) };
        if let Some(control) = host
            .control
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
        {
            control.update_scroll_position(id as NodeId, Point::new(Points(x), Points(y)));
        }
    }
    // Then dispatch the event for onScroll listeners
    dispatch(
        handle,
        id,
        event,
        EventPayload::Scroll {
            x,
            y,
            content_w,
            content_h,
            width,
            height,
        },
    );
}

/// # Safety
///
/// `handle` is live. `text` is the field's whole new value.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_input(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    text: JString<'_>,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    let text: String = env
        .with_env(|env| -> jni::errors::Result<String> {
            Ok(text.try_to_string(env).unwrap_or_default())
        })
        .resolve::<LogErrorAndDefault>();
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.input_changed(id as NodeId, text);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_selection(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    start: jint,
    end: jint,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.selection_changed(id as NodeId, start.max(0) as u32, end.max(0) as u32);
    }
}

/// # Safety
///
/// `handle` is live. `text` is a Java string.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_composition(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    composing: jboolean,
    text: JString<'_>,
) {
    if handle == 0 {
        return;
    }
    let text: String = env
        .with_env(|env| -> jni::errors::Result<String> {
            Ok(text.try_to_string(env).unwrap_or_default())
        })
        .resolve::<LogErrorAndDefault>();
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.composition_changed(id as NodeId, composing, text);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_frame(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    if let Some(control) = unsafe { host(handle) }
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.frame();
    }
}

fn dispatch(handle: jlong, id: jint, event: EventKind, payload: EventPayload) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.dispatch_event(id as NodeId, event, payload);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_screenHeader(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    height: jfloat,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.set_screen_header(id as NodeId, Some(Points(height)));
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_rootInsets(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    top: jfloat,
    right: jfloat,
    bottom: jfloat,
    left: jfloat,
) {
    dispatch(
        handle,
        id,
        EventKind::Insets,
        EventPayload::Insets {
            top,
            right,
            bottom,
            left,
        },
    );
}

/// # Safety
///
/// `handle` is live. Sizes and insets are points.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_rootSize(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    width: jfloat,
    height: jfloat,
    top: jfloat,
    right: jfloat,
    bottom: jfloat,
    left: jfloat,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    let control = host.control.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(control) = control.as_ref() {
        control.set_root_size(
            id as NodeId,
            Size::new(Points(width), Points(height)),
            Edges {
                top: Points(top),
                right: Points(right),
                bottom: Points(bottom),
                left: Points(left),
            },
        );
        control.dispatch_event(
            id as NodeId,
            EventKind::Insets,
            EventPayload::Insets {
                top,
                right,
                bottom,
                left,
            },
        );
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_modalShow(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    if handle == 0 {
        return;
    }
    dispatch(handle, id, EventKind::Show, EventPayload::None);
}

/// # Safety
///
/// `handle` is live. Gesture values are in points and points/second.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_panStart(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    crate::animation::pan_began(id as NodeId);
    dispatch(handle, id, EventKind::PanStart, EventPayload::None);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_panMove(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _handle: jlong,
    id: jint,
    tx: jfloat,
    ty: jfloat,
) {
    crate::animation::pan_moved(id as NodeId, tx, ty);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_panEnd(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    tx: jfloat,
    ty: jfloat,
    vx: jfloat,
    vy: jfloat,
) {
    let (x_val, y_val) = crate::animation::pan_values(id as NodeId);
    dispatch(
        handle,
        id,
        EventKind::PanEnd,
        EventPayload::Pan {
            translation: (tx, ty),
            velocity: (vx, vy),
            values: (x_val.unwrap_or(0.0), y_val.unwrap_or(0.0)),
        },
    );
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_pinchStart(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    crate::animation::transform_began(id as NodeId, false);
    dispatch(handle, id, EventKind::PinchStart, EventPayload::None);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_pinchMove(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _handle: jlong,
    id: jint,
    scale: jfloat,
) {
    crate::animation::transform_moved(id as NodeId, false, scale);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_pinchEnd(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    scale: jfloat,
    velocity: jfloat,
) {
    let value = crate::animation::transform_value(id as NodeId, false).unwrap_or(1.0);
    dispatch(
        handle,
        id,
        EventKind::PinchEnd,
        EventPayload::Pinch {
            scale,
            velocity,
            value,
        },
    );
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_rotationStart(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
) {
    crate::animation::transform_began(id as NodeId, true);
    dispatch(handle, id, EventKind::RotationStart, EventPayload::None);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_rotationMove(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _handle: jlong,
    id: jint,
    angle: jfloat,
) {
    crate::animation::transform_moved(id as NodeId, true, angle);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_rotationEnd(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    id: jint,
    angle: jfloat,
    velocity: jfloat,
) {
    let value = crate::animation::transform_value(id as NodeId, true).unwrap_or(0.0);
    dispatch(
        handle,
        id,
        EventKind::RotationEnd,
        EventPayload::Rotation {
            angle,
            velocity,
            value,
        },
    );
}

/// # Safety
///
/// `handle` is live. Returns true if the view has pan gesture bindings.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_hasPan(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _handle: jlong,
    id: jint,
) -> jboolean {
    crate::animation::has_pan(id as NodeId)
}

/// # Safety
///
/// `handle` is live. Computes snap target for scroll behavior. All lengths
/// are in pixels; this converts to points using the host's density.
/// Returns -1.0 if no snapping, otherwise the snap target in pixels.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_snapTarget(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    _id: jint,
    offset_px: jfloat,
    target_px: jfloat,
    velocity_px_per_ms: jfloat,
    viewport_px: jfloat,
    content_px: jfloat,
    paging: jboolean,
    snap_interval_pt: jfloat,
    snap_offsets_pt: JFloatArray<'_>,
    snap_alignment: jint,
    disable_interval_momentum: jboolean,
    deceleration: jfloat,
) -> jfloat {
    if handle == 0 {
        return -1.0;
    }
    let host = unsafe { host(handle) };
    let density = host.density.get();

    // Convert px to points
    let offset = Points(offset_px / density);
    let target = Points(target_px / density);
    let velocity = velocity_px_per_ms / density;
    let viewport = Points(viewport_px / density);
    let content = Points(content_px / density);

    // Extract snap_offsets array
    let snap_offsets = env
        .with_env(|env| -> jni::errors::Result<Vec<Points>> {
            if snap_offsets_pt.is_null() {
                return Ok(Vec::new());
            }
            let len = snap_offsets_pt.len(env)?;
            let mut values = vec![0.0f32; len];
            snap_offsets_pt.get_region(env, 0, &mut values)?;
            Ok(values.into_iter().map(Points).collect())
        })
        .resolve::<LogErrorAndDefault>();

    let snap_alignment = match snap_alignment {
        0 => nlayer_core::snap::SnapAlignment::Start,
        1 => nlayer_core::snap::SnapAlignment::Center,
        2 => nlayer_core::snap::SnapAlignment::End,
        _ => nlayer_core::snap::SnapAlignment::Start,
    };

    let behavior = nlayer_core::snap::SnapBehavior {
        paging: paging,
        snap_interval: if snap_interval_pt > 0.0 {
            Points(snap_interval_pt)
        } else {
            Points(0.0)
        },
        snap_offsets,
        snap_alignment,
        deceleration,
        one_step: disable_interval_momentum,
    };

    match nlayer_core::snap::snap_target(&behavior, offset, target, velocity, viewport, content) {
        Some(snap_pt) => snap_pt.0 * density,
        None => -1.0,
    }
}
