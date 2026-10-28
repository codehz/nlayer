//! One running surface: the engine thread and the UI-thread mounter.
//!
//! Extracted from `nsh-platform-android/src/host.rs` and cut to one surface,
//! press, input and safe area. There is no dev server, grant or animation
//! tick.
#![allow(unreachable_pub)]

use std::sync::{Arc, Mutex};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JObject, JString, JValue},
    sys::{jfloat, jint, jlong},
};
use nlayer_core::geometry::{Edges, Points, Size, px};
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::{Engine, EngineConfig, EngineControl, EventPayload, MountSink, Os};

use crate::encode::{self, Object};
use crate::java::{java, with_env};
use crate::text::{AndroidTextEngine, Density};

struct Host {
    view: jni::objects::GlobalRef,
    engine: Mutex<Option<Engine>>,
    control: Mutex<Option<EngineControl>>,
    density: Density,
    size: Mutex<Size<Points>>,
}

struct Sink {
    view: jni::objects::GlobalRef,
}

impl MountSink for Sink {
    fn submit(&self, batch: nlayer_core::MountBatch) {
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
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    code: JByteArray<'_>,
    _assets: JString<'_>,
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
    let density = host.density.clone();
    let sink = Arc::new(Sink {
        view: host.view.clone(),
    });
    match Engine::start(
        EngineConfig::new(bundle, viewport).with_os(Os::Android),
        move || AndroidTextEngine { density },
        sink,
    ) {
        Ok(engine) => {
            *host.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine.control());
            *host.engine.lock().unwrap_or_else(|err| err.into_inner()) = Some(engine);
        }
        Err(err) => report(&host.view, &err.message),
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
    let Some(control) = host
        .control
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    else {
        return;
    };
    control.set_safe_area(Edges {
        top: Points(top),
        right: Points(right),
        bottom: Points(bottom),
        left: Points(left),
    });
    control.set_keyboard_inset(Points(keyboard));
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
    dispatch(handle, id, EventKind::Press);
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

fn dispatch(handle: jlong, id: jint, event: EventKind) {
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
        control.dispatch_event(id as NodeId, event, EventPayload::None);
    }
}
