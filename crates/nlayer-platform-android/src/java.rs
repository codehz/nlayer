//! Cached Java classes and method ids.
//!
//! Classes are looked up once in `JNI_OnLoad`: threads created by Rust (the
//! engine thread) only see the system class loader, which cannot find the
//! app's classes.
//!
//! Extracted from `nsh-platform-android/src/java.rs` and cut to text
//! measurement and the one surface view.

use std::sync::OnceLock;

use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JClass, JMethodID, JStaticMethodID},
};

/// Everything Rust calls in Java.
pub(crate) struct Java {
    pub(crate) vm: JavaVM,
    /// `dev.nlayer.android.TextLayouts`.
    pub(crate) text: GlobalRef,
    pub(crate) text_prepare: JStaticMethodID,
    pub(crate) text_measure: JStaticMethodID,
    pub(crate) text_layout: JStaticMethodID,
    pub(crate) text_lines: JStaticMethodID,
    /// `dev.nlayer.android.HostView`.
    pub(crate) view_submit: JMethodID,
    pub(crate) view_report_error: JMethodID,
}

static JAVA: OnceLock<Java> = OnceLock::new();

/// The cached classes. Panics before `JNI_OnLoad`.
pub(crate) fn java() -> &'static Java {
    JAVA.get().expect("JNI_OnLoad")
}

pub(crate) fn init(vm: JavaVM) -> jni::errors::Result<()> {
    let mut env = vm.get_env()?;
    let class = |env: &mut JNIEnv<'_>, name: &str| -> jni::errors::Result<GlobalRef> {
        let class = env.find_class(name)?;
        env.new_global_ref(class)
    };
    let text = class(&mut env, "dev/nlayer/android/TextLayouts")?;
    let view = class(&mut env, "dev/nlayer/android/HostView")?;
    let text_class: &JClass<'_> = text.as_obj().into();
    let view_class: &JClass<'_> = view.as_obj().into();
    let java = Java {
        text_prepare: env.get_static_method_id(
            text_class,
            "prepare",
            "(Ljava/lang/String;[I[FIII)Ljava/lang/Object;",
        )?,
        text_measure: env.get_static_method_id(text_class, "measure", "(Ljava/lang/Object;F)J")?,
        text_layout: env.get_static_method_id(
            text_class,
            "layout",
            "(Ljava/lang/Object;FF)Ljava/lang/Object;",
        )?,
        text_lines: env.get_static_method_id(text_class, "lines", "(Ljava/lang/Object;F)[F")?,
        view_submit: env.get_method_id(view_class, "submit", "([B[Ljava/lang/Object;)V")?,
        view_report_error: env.get_method_id(view_class, "reportError", "(Ljava/lang/String;)V")?,
        text,
        vm,
    };
    let _ = JAVA.set(java);
    Ok(())
}

/// Runs `f` with this thread's JNI env, attaching it for good. A local
/// frame keeps references from piling up on the engine thread.
pub(crate) fn with_env<R: Default>(f: impl FnOnce(&mut JNIEnv<'_>) -> jni::errors::Result<R>) -> R {
    let java = java();
    let mut env = match java.vm.attach_current_thread_permanently() {
        Ok(env) => env,
        Err(err) => {
            log::error!("cannot attach to the JVM: {err}");
            return R::default();
        }
    };
    match env.with_local_frame(32, |env| f(env)) {
        Ok(value) => value,
        Err(err) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            log::error!("JNI call failed: {err}");
            R::default()
        }
    }
}
