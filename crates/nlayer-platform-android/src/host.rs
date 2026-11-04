//! One running surface: the engine thread and the UI-thread mounter.
//!
//! Extracted from `nsh-platform-android/src/host.rs` and cut to one surface,
//! press, input, scroll events and safe area. The dev server's client lives
//! in `dev.rs`.
#![allow(unreachable_pub)]

use std::sync::{Arc, Mutex};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jfloat, jint, jlong},
};
use nlayer_core::geometry::{Edges, Point, Points, Size, px};
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::{Engine, EngineConfig, EngineControl, EventPayload, MountSink, Os};

use crate::encode::{self, Object};
use crate::java::{java, with_env};
use crate::text::{AndroidTextEngine, Density};

pub(crate) struct Host {
    pub(crate) view: jni::objects::GlobalRef,
    pub(crate) engine: Mutex<Option<Engine>>,
    pub(crate) control: Mutex<Option<EngineControl>>,
    /// `app.js` the engine is running, after a start succeeded.
    pub(crate) boot_script: Mutex<String>,
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
    view: jni::objects::GlobalRef,
}

impl MountSink for Sink {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        crate::animation::note(&batch.items);
        let encoded = encode::encode(batch);
        with_env(|env| {
            let bytes = env.byte_array_from_slice(&encoded.bytes)?;
            let objects = env.new_object_array(
                encoded.objects.len() as i32,
                "java/lang/Object",
                JObject::null(),
            )?;
            for (index, object) in encoded.objects.iter().enumerate() {
                match object {
                    Object::Str(text) => {
                        let text = env.new_string(text)?;
                        env.set_object_array_element(&objects, index as i32, text)?;
                    }
                    Object::Layout(layout) => {
                        env.set_object_array_element(&objects, index as i32, layout.as_obj())?;
                    }
                }
            }
            // SAFETY: matches the signature checked in `java::init`.
            unsafe {
                env.call_method_unchecked(
                    self.view.as_obj(),
                    java().view_submit,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                    &[
                        JValue::Object(&bytes).as_jni(),
                        JValue::Object(&objects).as_jni(),
                    ],
                )
            }?;
            Ok(())
        });
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
            // SAFETY: matches the signature checked in `java::init`.
            unsafe {
                env.call_method_unchecked(
                    self.view.as_obj(),
                    java().view_back_unhandled,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                    &[],
                )
            }?;
            Ok(())
        });
    }

    fn request_frame(&self) -> bool {
        with_env(|env| {
            // SAFETY: matches the signature checked in `java::init`.
            unsafe {
                env.call_method_unchecked(
                    self.view.as_obj(),
                    java().view_request_frame,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                    &[],
                )
            }?;
            Ok(true)
        })
    }
}

fn report(view: &jni::objects::GlobalRef, message: &str) {
    with_env(|env| {
        let message = env.new_string(message)?;
        // SAFETY: matches the signature checked in `java::init`.
        unsafe {
            env.call_method_unchecked(
                view.as_obj(),
                java().view_report_error,
                jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                &[JValue::Object(&message).as_jni()],
            )
        }?;
        Ok(())
    });
}

/// Shows the development overlay over the app, or removes it when `message`
/// is `None`. Not part of the app tree: the view adds it as its own child,
/// outside the mounted views. Posts to the UI thread like [`report`].
pub(crate) fn overlay_view(view: &jni::objects::GlobalRef, message: Option<&str>) {
    with_env(|env| {
        let message = match message {
            Some(message) => env.new_string(message)?.into(),
            None => JObject::null(),
        };
        // SAFETY: matches the signature checked in `java::init`.
        unsafe {
            env.call_method_unchecked(
                view.as_obj(),
                java().view_overlay,
                jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                &[JValue::Object(&message).as_jni()],
            )
        }?;
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
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    view: JObject<'_>,
    density: jfloat,
) -> jlong {
    let view = match env.new_global_ref(view) {
        Ok(view) => view,
        Err(err) => {
            log::error!("nlayer: cannot retain the surface: {err}");
            return 0;
        }
    };
    let host = Arc::new(Host {
        view,
        engine: Mutex::new(None),
        control: Mutex::new(None),
        boot_script: Mutex::new(String::new()),
        launch: Mutex::new(None),
        density: Density::new(density),
        size: Mutex::new(Size::new(px(0.0), px(0.0))),
        insets: Mutex::new(None),
    });
    Box::into_raw(Box::new(host)) as jlong
}

/// # Safety
///
/// `handle` is a live pointer from [`Java_dev_nlayer_android_Native_create`],
/// or 0.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_destroy(
    _env: JNIEnv<'_>,
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
    mut env: JNIEnv<'_>,
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
    let bytes = match env.convert_byte_array(code) {
        Ok(bytes) => bytes,
        Err(err) => {
            if quiet == 0 {
                report(&host.view, &format!("cannot read the bundle: {err}"));
            } else {
                log::error!("nlayer: cannot read the bundle: {err}");
            }
            return;
        }
    };
    let bundle = String::from_utf8_lossy(&bytes).into_owned();
    let dir = env
        .get_string(&data_dir)
        .map(|dir| String::from(dir))
        .unwrap_or_default();
    *host.launch.lock().unwrap_or_else(|err| err.into_inner()) = Some(Launch {
        data_dir: dir.clone(),
        dark: dark != 0,
    });
    match start_engine(host, &bundle, &dir, dark != 0) {
        Ok(()) => {
            *host
                .boot_script
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = bundle;
        }
        Err(err) => {
            if quiet == 0 {
                report(&host.view, &err);
            } else {
                log::error!("nlayer: {err}");
            }
        }
    }
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
        view: host.view.clone(),
    });
    let mut config = EngineConfig::new(bundle, viewport)
        .with_os(Os::Android)
        // Android's toolbar is 56dp; it has no large titles.
        .with_header_heights((Points(56.0), Points(56.0)))
        .with_color_scheme(nlayer_core::ColorScheme::from_dark(dark));
    if !data_dir.is_empty() {
        config = config.with_data_dir(data_dir);
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
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    address: JString<'_>,
    port: jint,
    test: jboolean,
) {
    if handle == 0 || port <= 0 || port > u16::MAX as jint {
        return;
    }
    let host = unsafe { host(handle) }.clone();
    let Ok(address) = env.get_string(&address) else {
        return;
    };
    crate::dev::start(host, address.into(), port as u16, test != 0);
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_animating(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jboolean {
    if handle == 0 {
        return 0;
    }
    crate::animation::animating() as jboolean
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_tick<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    nanos: jlong,
) -> jni::sys::jbyteArray {
    if handle == 0 {
        return std::ptr::null_mut();
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
    match env.byte_array_from_slice(&encoded.bytes) {
        Ok(array) => array.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_resize(
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
        control.set_color_scheme(nlayer_core::ColorScheme::from_dark(dark != 0));
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_back(
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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

/// # Safety
///
/// `handle` is live. `address` is the dev server (`127.0.0.1` after
/// `adb reverse`). The host connects `ws://address:port/ws`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_serve(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    address: JString<'_>,
    port: jint,
) {
    if handle == 0 || port == 0 {
        return;
    }
    let host = unsafe { host(handle) }.clone();
    let address: String = env
        .get_string(&address)
        .map(|value| value.into())
        .unwrap_or_default();
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    mut env: JNIEnv<'_>,
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
        .get_string(&text)
        .map(|value| value.into())
        .unwrap_or_default();
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
    _env: JNIEnv<'_>,
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
    mut env: JNIEnv<'_>,
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
        .get_string(&text)
        .map(|value| value.into())
        .unwrap_or_default();
    let host = unsafe { host(handle) };
    if let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
    {
        control.composition_changed(id as NodeId, composing != 0, text);
    }
}

/// # Safety
///
/// `handle` is live.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_frame(
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
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
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    _handle: jlong,
    id: jint,
) -> jboolean {
    crate::animation::has_pan(id as NodeId) as jboolean
}

/// # Safety
///
/// `handle` is live. Computes snap target for scroll behavior. All lengths
/// are in pixels; this converts to points using the host's density.
/// Returns -1.0 if no snapping, otherwise the snap target in pixels.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_snapTarget(
    mut env: JNIEnv<'_>,
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
    snap_offsets_pt: jni::sys::jfloatArray,
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
    let snap_offsets = if snap_offsets_pt.is_null() {
        Vec::new()
    } else {
        let arr = unsafe { jni::objects::JFloatArray::from_raw(snap_offsets_pt) };
        // SAFETY: `snap_offsets_pt` is a non-null `float[]` from this JNI
        // call. The elements are copied out before the guard drops, and
        // `NoCopyBack` does not write them back.
        match unsafe { env.get_array_elements(&arr, jni::objects::ReleaseMode::NoCopyBack) } {
            Ok(elements) => {
                let slice =
                    unsafe { std::slice::from_raw_parts(elements.as_ptr(), elements.len()) };
                slice.iter().map(|&v| Points(v)).collect()
            }
            Err(_) => Vec::new(),
        }
    };

    let snap_alignment = match snap_alignment {
        0 => nlayer_core::snap::SnapAlignment::Start,
        1 => nlayer_core::snap::SnapAlignment::Center,
        2 => nlayer_core::snap::SnapAlignment::End,
        _ => nlayer_core::snap::SnapAlignment::Start,
    };

    let behavior = nlayer_core::snap::SnapBehavior {
        paging: paging != 0,
        snap_interval: if snap_interval_pt > 0.0 {
            Points(snap_interval_pt)
        } else {
            Points(0.0)
        },
        snap_offsets,
        snap_alignment,
        deceleration,
        one_step: disable_interval_momentum != 0,
    };

    match nlayer_core::snap::snap_target(&behavior, offset, target, velocity, viewport, content) {
        Some(snap_pt) => snap_pt.0 * density,
        None => -1.0,
    }
}
