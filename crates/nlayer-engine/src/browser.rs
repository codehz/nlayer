//! A page-owned script. The browser evaluates the app's modules; this
//! engine only applies the ops that evaluation queued. Nothing here parses
//! or runs JavaScript.

use std::{cell::RefCell, rc::Rc};

use nlayer_core::{EventKind, NodeId, Op};

use crate::host_types::{EventPayload, HttpResponse, NetworkRequest, Os};

/// What the page's runtime does for one engine turn.
pub trait Bridge {
    /// Moves ops queued since the last call into `out`.
    fn take_ops(&self, out: &mut Vec<Op>);
    /// Delivers one view event to the page.
    fn dispatch_event(&self, id: NodeId, event: EventKind, payload: &EventPayload);
    /// `platform.os` for this surface. The web runtime reports `"web"` itself.
    fn set_os(&self, os: Os);
}

thread_local! {
    static NEXT: RefCell<Option<Rc<dyn Bridge>>> = const { RefCell::new(None) };
}

/// Supplies the bridge [`Script::start`] consumes. A second call before
/// startup is an error, never a replacement.
pub fn register(host: Rc<dyn Bridge>) -> Result<(), String> {
    NEXT.with(|slot| {
        let mut cur = slot.borrow_mut();
        if cur.is_some() {
            return Err("a browser host is already awaiting startup".into());
        }
        *cur = Some(host);
        Ok(())
    })
}

/// Drops a registration startup did not consume.
pub fn clear_registration() {
    NEXT.with(|slot| *slot.borrow_mut() = None);
}

/// The engine's script when the page is the JavaScript host.
pub(crate) struct Script(Rc<dyn Bridge>);

impl Script {
    pub(crate) fn start(os: Os) -> Result<Self, String> {
        let host = NEXT
            .with(|slot| slot.borrow_mut().take())
            .ok_or_else(|| "no browser host was registered".to_owned())?;
        host.set_os(os);
        Ok(Script(host))
    }

    pub(crate) fn take_ops(&self, out: &mut Vec<Op>) {
        self.0.take_ops(out);
    }

    pub(crate) fn dispatch_event(&self, id: NodeId, event: EventKind, payload: &EventPayload) {
        self.0.dispatch_event(id, event, payload);
    }

    pub(crate) fn next_timer(&self) -> Option<std::time::Duration> {
        None
    }

    pub(crate) fn run_due_timers(&self) {}

    pub(crate) fn take_network(&self, out: &mut Vec<NetworkRequest>) {
        let _ = out;
    }

    pub(crate) fn complete_network(&self, id: u32, result: Result<HttpResponse, String>) {
        let _ = (id, result);
    }
}
