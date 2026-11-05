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
    Env, JavaVM,
    ids::{JMethodID, JStaticMethodID},
    jni_sig, jni_str,
    objects::JClass,
    refs::Global,
};

/// Everything Rust calls in Java.
pub(crate) struct Java {
    pub(crate) vm: JavaVM,
    /// `dev.nlayer.android.TextLayouts`.
    pub(crate) text: Global<JClass<'static>>,
    pub(crate) text_prepare: JStaticMethodID,
    pub(crate) text_measure: JStaticMethodID,
    pub(crate) text_layout: JStaticMethodID,
    pub(crate) text_lines: JStaticMethodID,
    pub(crate) text_attachments: JStaticMethodID,
    pub(crate) text_hit_test: JStaticMethodID,
    /// `dev.nlayer.android.HostView`.
    pub(crate) view_submit: JMethodID,
    pub(crate) view_report_error: JMethodID,
    pub(crate) view_overlay: JMethodID,
    pub(crate) view_snapshot: JMethodID,
    pub(crate) view_tap: JMethodID,
    pub(crate) view_move: JMethodID,
    pub(crate) view_back: JMethodID,
    pub(crate) view_type_char: JMethodID,
    pub(crate) view_scroll_to: JMethodID,
    pub(crate) view_hover: JMethodID,
    pub(crate) view_back_unhandled: JMethodID,
    pub(crate) view_request_frame: JMethodID,
    pub(crate) view_recycle: JMethodID,
}

static JAVA: OnceLock<Java> = OnceLock::new();

/// The cached classes. Panics before `JNI_OnLoad`.
pub(crate) fn java() -> &'static Java {
    JAVA.get().expect("JNI_OnLoad")
}

pub(crate) fn init(vm: JavaVM) -> jni::errors::Result<()> {
    let java = vm.attach_current_thread(|env| -> jni::errors::Result<Java> {
        let text = env.find_class(jni_str!("dev/nlayer/android/TextLayouts"))?;
        let view_class = env.find_class(jni_str!("dev/nlayer/android/HostView"))?;
        let text: Global<JClass<'static>> = env.new_global_ref(text)?;
        let view_class: Global<JClass<'static>> = env.new_global_ref(view_class)?;
        Ok(Java {
            vm: vm.clone(),
            text_prepare: env.get_static_method_id(
                &text,
                jni_str!("prepare"),
                jni_sig!("(Ljava/lang/String;[I[FIIIF[F)Ljava/lang/Object;"),
            )?,
            text_measure: env.get_static_method_id(
                &text,
                jni_str!("measure"),
                jni_sig!("(Ljava/lang/Object;F)J"),
            )?,
            text_layout: env.get_static_method_id(
                &text,
                jni_str!("layout"),
                jni_sig!("(Ljava/lang/Object;FF)Ljava/lang/Object;"),
            )?,
            text_lines: env.get_static_method_id(
                &text,
                jni_str!("lines"),
                jni_sig!("(Ljava/lang/Object;F)[F"),
            )?,
            text_attachments: env.get_static_method_id(
                &text,
                jni_str!("attachments"),
                jni_sig!("(Ljava/lang/Object;F)[F"),
            )?,
            text_hit_test: env.get_static_method_id(
                &text,
                jni_str!("hitTest"),
                jni_sig!("(Ljava/lang/Object;FFF)J"),
            )?,
            view_submit: env.get_method_id(
                &view_class,
                jni_str!("submit"),
                jni_sig!("([B[Ljava/lang/Object;)V"),
            )?,
            view_report_error: env.get_method_id(
                &view_class,
                jni_str!("reportError"),
                jni_sig!("(Ljava/lang/String;)V"),
            )?,
            view_overlay: env.get_method_id(
                &view_class,
                jni_str!("overlay"),
                jni_sig!("(Ljava/lang/String;)V"),
            )?,
            view_snapshot: env.get_method_id(
                &view_class,
                jni_str!("snapshot"),
                jni_sig!("()Ljava/lang/String;"),
            )?,
            view_tap: env.get_method_id(&view_class, jni_str!("tap"), jni_sig!("(FFZ)V"))?,
            view_move: env.get_method_id(&view_class, jni_str!("move"), jni_sig!("(FF)V"))?,
            view_back: env.get_method_id(&view_class, jni_str!("back"), jni_sig!("()Z"))?,
            view_type_char: env.get_method_id(
                &view_class,
                jni_str!("typeChar"),
                jni_sig!("(ILjava/lang/String;)V"),
            )?,
            view_scroll_to: env.get_method_id(
                &view_class,
                jni_str!("scrollTo"),
                jni_sig!("(IFF)V"),
            )?,
            view_hover: env.get_method_id(&view_class, jni_str!("hover"), jni_sig!("(FF)V"))?,
            view_back_unhandled: env.get_method_id(
                &view_class,
                jni_str!("backUnhandled"),
                jni_sig!("()V"),
            )?,
            view_request_frame: env.get_method_id(
                &view_class,
                jni_str!("requestFrame"),
                jni_sig!("()V"),
            )?,
            view_recycle: env.get_method_id(
                &view_class,
                jni_str!("recycle"),
                jni_sig!("(FFFFFFFLjava/lang/String;)V"),
            )?,
            text,
        })
    })?;
    let _ = JAVA.set(java);
    Ok(())
}

/// Runs `f` with this thread's JNI env, attaching it for good. The
/// attachment's local frame keeps references from piling up on the engine
/// thread.
pub(crate) fn with_env<R: Default>(f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<R>) -> R {
    match java().vm.attach_current_thread(f) {
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
    java()
        .vm
        .attach_current_thread(f)
        .map_err(|error| error.to_string())
}
