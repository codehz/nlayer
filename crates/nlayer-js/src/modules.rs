//! The native ES modules an app imports: `@nlayer/runtime` (the reactive
//! graph, `mount`, `context`, `ErrorBoundary`) and `@nlayer/ui` (the host
//! components and `platform.os`). `nlayer:rt` is the renderer's, which only
//! compiled JSX imports. `@nlayer/jsx-runtime` is types only and is not
//! served here.
//!
//! The exports are made when a bundle first imports the module. QuickJS
//! resolves imports when it compiles a module, so the loader declares the
//! export names then.

use std::ffi::{CStr, c_char, c_int};

use rquickjs::{Ctx, Module, qjs};

use crate::{
    dom,
    reactive::{self, Native},
};

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

/// A value a native module exports.
#[derive(Clone, Copy)]
pub(crate) enum Export {
    /// A native function and its `length`.
    Function(Native, c_int),
    /// A value made when the module is first imported (owned).
    Value(fn(*mut JsContext) -> JsValue),
}

/// An export: its name and how it is made.
pub(crate) type Item = (&'static str, Export);

/// One of nlayer's own modules.
pub(crate) struct NativeModule {
    pub(crate) name: &'static str,
    pub(crate) exports: &'static [Item],
}

/// `View` as a component: builds the host element and its children.
fn view(cx: *mut JsContext) -> JsValue {
    // SAFETY: a live context.
    dom::host_component(cx, "view", true)
}

fn text(cx: *mut JsContext) -> JsValue {
    dom::host_component(cx, "text", true)
}

fn image(cx: *mut JsContext) -> JsValue {
    dom::host_component(cx, "image", false)
}

fn scroll(cx: *mut JsContext) -> JsValue {
    dom::host_component(cx, "scroll", true)
}

fn input(cx: *mut JsContext) -> JsValue {
    dom::host_component(cx, "input", false)
}

/// `List` as a component: creates the list and its recycling cells.
fn list(cx: *mut JsContext) -> JsValue {
    reactive::new_function(cx, c"List", crate::list::list, 1)
}

/// `platform`: `{ os, colorScheme }`. Safe area and the keyboard inset are
/// not on it; padding and margin read them through `env()`.
fn platform(cx: *mut JsContext) -> JsValue {
    // SAFETY: the state of this context; the object is new and the module takes it.
    unsafe {
        let os = (*crate::state::state(cx)).os.get().name();
        let object = qjs::JS_NewObject(cx);
        let name = qjs::JS_NewStringLen(cx, os.as_ptr().cast(), os.len() as _);
        qjs::JS_SetPropertyStr(cx, object, c"os".as_ptr(), name);
        let scheme = reactive::new_function(cx, c"colorScheme", color_scheme, 0);
        qjs::JS_SetPropertyStr(cx, object, c"colorScheme".as_ptr(), scheme);
        object
    }
}

/// `platform.colorScheme()`: the system's scheme. Reading it inside an
/// effect or a style follows a change.
unsafe extern "C" fn color_scheme(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
) -> JsValue {
    // SAFETY: the context's signal, called on the live context; the result
    // is owned.
    unsafe {
        let signal = (*crate::state::state(cx)).color_scheme.get();
        qjs::JS_Call(cx, signal, qjs::JS_UNDEFINED, 0, std::ptr::null_mut())
    }
}

/// `BackHandler`: the prelude's listener table.
fn back_handler(cx: *mut JsContext) -> JsValue {
    // SAFETY: the prelude's object on the global; the result is new.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        let value = qjs::JS_GetPropertyStr(cx, global, c"__nlayerBackHandler".as_ptr());
        qjs::JS_FreeValue(cx, global);
        value
    }
}

/// `storage`: the app's key-value storage the prelude made.
fn storage(cx: *mut JsContext) -> JsValue {
    // SAFETY: the prelude's object on the global; the result is new.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        let value = qjs::JS_GetPropertyStr(cx, global, c"__nlayerStorage".as_ptr());
        qjs::JS_FreeValue(cx, global);
        value
    }
}

/// `createAnimatedValue`: the animated-value prelude's constructor.
fn animated_value(cx: *mut JsContext) -> JsValue {
    // SAFETY: the prelude's object on the global; the property is new.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        let animated = qjs::JS_GetPropertyStr(cx, global, c"__nlayerAnimated".as_ptr());
        let value = qjs::JS_GetPropertyStr(cx, animated, c"createAnimatedValue".as_ptr());
        qjs::JS_FreeValue(cx, animated);
        qjs::JS_FreeValue(cx, global);
        value
    }
}

/// `Animated`: `{ timing, spring }`.
fn animated(cx: *mut JsContext) -> JsValue {
    // SAFETY: the prelude's object on the global; the property is new.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        let animated = qjs::JS_GetPropertyStr(cx, global, c"__nlayerAnimated".as_ptr());
        let value = qjs::JS_GetPropertyStr(cx, animated, c"Animated".as_ptr());
        qjs::JS_FreeValue(cx, animated);
        qjs::JS_FreeValue(cx, global);
        value
    }
}

/// `Keyboard`: `{ dismiss() }`. The call is `Command::Blur` on the root.
fn keyboard(cx: *mut JsContext) -> JsValue {
    // SAFETY: a new object; the module takes the function.
    unsafe {
        let object = qjs::JS_NewObject(cx);
        let dismiss = reactive::new_function(cx, c"dismiss", dom::dismiss_keyboard, 0);
        qjs::JS_SetPropertyStr(cx, object, c"dismiss".as_ptr(), dismiss);
        object
    }
}

/// `@nlayer/runtime`: the reactive graph and the tree entry an app mounts.
const RUNTIME: &[Item] = &[
    ("signal", Export::Function(reactive::signal, 2)),
    ("computed", Export::Function(reactive::computed, 2)),
    ("effect", Export::Function(reactive::effect, 1)),
    ("batch", Export::Function(reactive::batch, 1)),
    ("untracked", Export::Function(reactive::untrack, 1)),
    ("cleanup", Export::Function(reactive::on_cleanup, 1)),
    ("context", Export::Function(dom::context, 1)),
    ("mount", Export::Function(dom::mount, 2)),
    ("ErrorBoundary", Export::Function(dom::error_boundary, 1)),
    ("resource", Export::Function(crate::resource::resource, 2)),
    ("storage", Export::Value(storage)),
];

/// `@nlayer/ui`: the host components.
const UI: &[Item] = &[
    ("View", Export::Value(view)),
    ("Text", Export::Value(text)),
    ("Image", Export::Value(image)),
    ("Scroll", Export::Value(scroll)),
    ("Input", Export::Value(input)),
    ("List", Export::Value(list)),
    ("createAnimatedValue", Export::Value(animated_value)),
    ("Animated", Export::Value(animated)),
    ("platform", Export::Value(platform)),
    ("BackHandler", Export::Value(back_handler)),
    ("Keyboard", Export::Value(keyboard)),
];

pub(crate) const MODULES: &[NativeModule] = &[
    NativeModule {
        name: "@nlayer/runtime",
        exports: RUNTIME,
    },
    NativeModule {
        name: "@nlayer/ui",
        exports: UI,
    },
    NativeModule {
        name: "nlayer:rt",
        exports: dom::RT_EXPORTS,
    },
];

/// Resolves a module name to the name the loader is asked for. Only bare
/// names are native modules: anything else comes from the bundle.
pub(crate) struct Resolver;
impl rquickjs::loader::Resolver for Resolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        _base: &str,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        Ok(name.to_owned())
    }
}

pub(crate) struct Loader;
impl rquickjs::loader::Loader for Loader {
    fn load<'js>(
        &mut self,
        cx: &Ctx<'js>,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        // SAFETY: the thunk returns exactly the native module declared for
        // this name, or throws.
        unsafe { Module::from_load_fn(cx.clone(), name, load_native) }
    }
}

unsafe extern "C" fn load_native(
    raw: *mut JsContext,
    name: *const c_char,
) -> *mut qjs::JSModuleDef {
    // SAFETY: called by `Module::from_load_fn` while its runtime is locked;
    // the temporary `Ctx` never escapes this callback.
    unsafe {
        let name = CStr::from_ptr(name);
        let module = MODULES
            .iter()
            .find(|m| m.name.as_bytes() == name.to_bytes());
        let Some(module) = module else {
            // Throws on the live context of this callback; `%s` keeps the
            // name literal.
            let name = std::ffi::CString::new(name.to_bytes()).unwrap_or_default();
            qjs::JS_ThrowReferenceError(
                raw,
                c"could not load module '%s' (only the runtime's native modules can be imported at run time)"
                    .as_ptr(),
                name.as_ptr(),
            );
            return std::ptr::null_mut();
        };
        let m = qjs::JS_NewCModule(raw, name.as_ptr(), Some(init));
        if m.is_null() {
            return m;
        }
        for (export, _) in module.exports {
            let export = std::ffi::CString::new(*export).unwrap_or_default();
            qjs::JS_AddModuleExport(raw, m, export.as_ptr());
        }
        m
    }
}

/// Fills a module's exports: `0`, or `-1` with an exception pending.
unsafe extern "C" fn init(cx: *mut JsContext, m: *mut qjs::JSModuleDef) -> c_int {
    // SAFETY: the module's name atom, freed below; exports are new values
    // the module takes.
    unsafe {
        let atom = qjs::JS_GetModuleName(cx, m);
        let name = qjs::JS_AtomToCStringLen(cx, std::ptr::null_mut(), atom);
        qjs::JS_FreeAtom(cx, atom);
        if name.is_null() {
            return -1;
        }
        let module_name = CStr::from_ptr(name).to_string_lossy().into_owned();
        qjs::JS_FreeCString(cx, name);
        let Some(module) = MODULES.iter().find(|m| m.name == module_name) else {
            return -1;
        };
        for (export, made) in module.exports {
            let value = match made {
                Export::Function(f, length) => {
                    let cname = std::ffi::CString::new(*export).unwrap_or_default();
                    reactive::new_function(cx, &cname, *f, *length)
                }
                Export::Value(make) => make(cx),
            };
            if qjs::JS_IsException(value) {
                return -1;
            }
            let cname = std::ffi::CString::new(*export).unwrap_or_default();
            if qjs::JS_SetModuleExport(cx, m, cname.as_ptr(), value) != 0 {
                return -1;
            }
        }
    }
    0
}

/// The names native module `specifier` exports, if it is one (a bundler
/// checks named imports against them).
pub fn native_module_exports(specifier: &str) -> Option<Vec<&'static str>> {
    MODULES
        .iter()
        .find(|m| m.name == specifier)
        .map(|m| m.exports.iter().map(|(name, _)| *name).collect())
}
