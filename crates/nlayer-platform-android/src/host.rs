//! One running surface: the engine thread and the UI-thread mounter.
//!
//! Extracted from `nsh-platform-android/src/host.rs` and cut to one surface,
//! press, input, scroll events and safe area. There is no dev server, grant
//! or animation tick.
#![allow(unreachable_pub)]

use std::sync::{Arc, Mutex};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jfloat, jint, jlong},
};
use nlayer_core::geometry::{Edges, Points, Size, px};
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::{Engine, EngineConfig, EngineControl, EventPayload, MountSink, Os};

use crate::encode::{self, Object};
use crate::java::{java, with_env};
use crate::text::{AndroidTextEngine, Density};

pub(crate) struct Host {
    pub(crate) view: jni::objects::GlobalRef,
    engine: Mutex<Option<Engine>>,
    control: Mutex<Option<EngineControl>>,
    density: Density,
    size: Mutex<Size<Points>>,
    insets: Mutex<Option<(Edges<Points>, Points)>>,
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
/// `handle` is live. `code` is a UTF-8 bundle.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_nlayer_android_Native_load(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    code: JByteArray<'_>,
    _assets: JString<'_>,
    data_dir: JString<'_>,
    dark: jboolean,
) {
    if handle == 0 {
        return;
    }
    let host = unsafe { host(handle) };
    let bytes = match env.convert_byte_array(code) {
        Ok(bytes) => bytes,
        Err(err) => {
            report(&host.view, &format!("cannot read the bundle: {err}"));
            return;
        }
    };
    let bundle = String::from_utf8_lossy(&bytes).into_owned();
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
        .with_color_scheme(nlayer_core::ColorScheme::from_dark(dark != 0));
    if let Ok(dir) = env.get_string(&data_dir) {
        config = config.with_data_dir(String::from(dir));
    }
    match Engine::start(config, move || AndroidTextEngine { density }, sink) {
        Ok(engine) => {
            *host.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine.control());
            *host.engine.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine);
            apply_insets(host);
        }
        Err(err) => report(&host.view, &err.message),
    }
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
/// `handle` is live. `address` is the test runner (`127.0.0.1` after
/// `adb reverse`).
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
    crate::driver::start(host, format!("{address}:{port}"));
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
