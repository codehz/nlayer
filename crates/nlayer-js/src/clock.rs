//! Time for timers and `performance.now`.
//!
//! [`std::time::Instant`] panics on `wasm32-unknown-unknown`: that target
//! has no OS clock. The QuickJS wasm shim imports `env.__rquickjs_host_now_us`
//! (microseconds since the Unix epoch, an `f64`) for `gettimeofday`, and
//! this module uses the same import. It is wall time, so a jump in the
//! page's clock moves timers. Native builds are [`std::time::Instant`].

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::Instant as Mono;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{ops::Add, time::Duration};

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn __rquickjs_host_now_us() -> f64;
    }

    /// Microseconds since the Unix epoch, from the page.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
    pub(crate) struct Mono(u64);

    impl Mono {
        pub(crate) fn now() -> Self {
            // SAFETY: the page returns the clock synchronously.
            Self(unsafe { __rquickjs_host_now_us() } as u64)
        }

        pub(crate) fn elapsed(self) -> Duration {
            Self::now().saturating_duration_since(self)
        }

        pub(crate) fn saturating_duration_since(self, earlier: Self) -> Duration {
            Duration::from_micros(self.0.saturating_sub(earlier.0))
        }
    }

    impl Add<Duration> for Mono {
        type Output = Self;

        fn add(self, rhs: Duration) -> Self {
            Self(self.0.saturating_add(rhs.as_micros() as u64))
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::Mono;
