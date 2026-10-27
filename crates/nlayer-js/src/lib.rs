//! QuickJS host for nlayer.
//!
//! [`Host`] owns one QuickJS runtime and context and serves the native
//! modules an app imports (`@nlayer`; `nlayer:rt` arrives with the JSX
//! runtime). A host is single-threaded (`!Send`): create and use it on the
//! engine thread.

mod error;
mod modules;
mod reactive;
mod state;

use rquickjs::{CatchResultExt, Context, Module, Persistent, Promise, Runtime};

pub use crate::error::JsError;
pub use crate::modules::native_module_exports;

/// Name of the app bundle's module (stack traces, imports it awaits).
pub const BUNDLE_NAME: &str = "bundle.js";

/// A QuickJS VM running one app.
pub struct Host {
    rt: Runtime,
    cx: Context,
}

impl Host {
    /// Makes a VM with the native modules installed.
    pub fn new() -> Result<Host, JsError> {
        let rt = Runtime::new()?;
        rt.set_loader(modules::Resolver, modules::Loader);
        let cx = Context::full(&rt)?;
        let installed = cx.with(|cx| -> Result<(), JsError> {
            state::install(&cx);
            Ok(())
        });
        installed?;
        Ok(Host { rt, cx })
    }

    /// Evaluates the app bundle (an ES module) and runs its jobs: its
    /// synchronous part and what its microtasks reach.
    pub fn eval_source(&self, source: &str) -> Result<(), JsError> {
        let promise = self.cx.with(|cx| -> Result<_, JsError> {
            let module = Module::declare(cx.clone(), BUNDLE_NAME, source)
                .catch(&cx)
                .map_err(JsError::caught)?;
            let (_, promise) = module.eval().catch(&cx).map_err(JsError::caught)?;
            Ok(Persistent::save(&cx, promise))
        })?;
        self.settle(promise)
    }

    /// Runs pending promise jobs and microtasks until the queue is empty.
    /// Every call from Rust into JS ends with it: it is the end of the turn.
    pub fn run_jobs(&self) {
        while let Ok(true) = self.rt.execute_pending_job() {}
    }

    /// Runs jobs until `promise` settles (or has no jobs left to run), then
    /// reports its failure.
    fn settle(&self, promise: Persistent<Promise<'static>>) -> Result<(), JsError> {
        self.run_jobs();
        self.cx.with(|cx| -> Result<(), JsError> {
            let promise = promise
                .restore(&cx)
                .map_err(|e| JsError::from_engine(&cx, e))?;
            match promise.result::<()>() {
                Some(result) => result.catch(&cx).map_err(JsError::caught),
                // Waiting on native work; the engine's next turn settles it.
                None => Ok(()),
            }
        })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.cx.with(|cx| state::uninstall(&cx));
    }
}
