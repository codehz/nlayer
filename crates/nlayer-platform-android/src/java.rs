//! Java bindings, VM setup and thread attachment.
//!
//! The bindings cache their class and method ids on first use. `JNI_OnLoad`
//! primes both caches on the thread the VM passes: threads created by Rust
//! (the engine thread) only see the system class loader, which cannot find
//! the app's classes.
//!
//! Extracted from `nsh-platform-android/src/java.rs` and cut to text
//! measurement and the one surface view.

use jni::{Env, JavaVM, bind_java_type, refs::LoaderContext};

bind_java_type! {
    pub(crate) TextLayouts => dev.nlayer.android.TextLayouts,
    methods {
        static fn prepare(
            text: JString,
            ints: [jint],
            floats: [jfloat],
            align: jint,
            max_lines: jint,
            ellipsize: jint,
            density: jfloat,
            attachments: [jfloat],
        ) -> JObject,
        static fn measure(prepared: JObject, max_width: jfloat) -> jlong,
        static fn layout(prepared: JObject, width: jfloat, height: jfloat) -> JObject,
        static fn lines(layout: JObject, density: jfloat) -> [jfloat],
        static fn attachments(layout: JObject, density: jfloat) -> [jfloat],
        static fn hit_test(layout: JObject, x: jfloat, y: jfloat, density: jfloat) -> jlong,
    },
}

bind_java_type! {
    pub(crate) HostView => dev.nlayer.android.HostView,
    methods {
        fn submit(bytes: [jbyte], objects: [JObject]),
        fn report_error(message: JString),
        fn overlay(message: JString),
        fn recycle(
            width: jfloat,
            height: jfloat,
            keyboard: jfloat,
            top: jfloat,
            right: jfloat,
            bottom: jfloat,
            left: jfloat,
            scheme: JString,
        ),
        fn snapshot() -> JString,
        fn tap(x: jfloat, y: jfloat, up: jboolean),
        fn move_view {
            sig = (x: jfloat, y: jfloat),
            name = "move",
        },
        fn back() -> jboolean,
        fn type_char(id: jint, text: JString),
        fn scroll_to(id: jint, x: jfloat, y: jfloat),
        fn hover(x: jfloat, y: jfloat),
        fn back_unhandled(),
        fn request_frame(),
    },
}

/// Loads both classes and their method ids on the `JNI_OnLoad` thread.
pub(crate) fn init(vm: JavaVM) -> jni::errors::Result<()> {
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        TextLayoutsAPI::get(env, &LoaderContext::None)?;
        HostViewAPI::get(env, &LoaderContext::None)?;
        Ok(())
    })
}

/// Runs `f` with this thread's JNI env, attaching it for good. The
/// attachment's local frame keeps references from piling up on the engine
/// thread.
pub(crate) fn with_env<R: Default>(f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<R>) -> R {
    let result = JavaVM::singleton().and_then(|vm| vm.attach_current_thread(f));
    match result {
        Ok(value) => value,
        Err(err) => {
            log::error!("JNI call failed: {err}");
            R::default()
        }
    }
}

/// [`with_env`] that returns the failure instead of a default.
pub(crate) fn try_with_env<R>(
    f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<R>,
) -> Result<R, String> {
    JavaVM::singleton()
        .and_then(|vm| vm.attach_current_thread(f))
        .map_err(|error| error.to_string())
}
