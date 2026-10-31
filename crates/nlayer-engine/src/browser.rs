//! A page-owned script. The browser evaluates the app's modules; this
//! engine only applies the ops that evaluation queued. Nothing here parses
//! or runs JavaScript.

use std::{cell::RefCell, rc::Rc};

use nlayer_core::{EventKind, NodeId, Op};

use crate::host_types::{EventPayload, Os};

/// What the page's runtime does for one engine turn.
pub trait Bridge {
    /// Moves ops queued since the last call into `out`.
    fn take_ops(&self, out: &mut Vec<Op>);
    /// Delivers one view event to the page.
    fn dispatch_event(&self, id: NodeId, event: EventKind, payload: &EventPayload);
    /// Renders the list cells the core asked for.
    fn bind_list(&self, list: NodeId, binds: &[(u32, u32)]);
    /// `platform.os` for this surface. The web runtime reports `"web"` itself.
    fn set_os(&self, os: Os);
    /// Native animations finished or were interrupted. The page resolves
    /// its own animation promises, so it usually has nothing to do.
    fn animations_finished(&self, _finished: &[nlayer_core::animation::Finished]) {}
    /// Delivers the platform's back button (or history back): whether a
    /// `BackHandler` listener handled it. Returns true if consumed.
    fn dispatch_back(&self) -> bool {
        false
    }
    /// Reduce motion changed (internal, not exported to apps). The web
    /// runtime computes its own from a media query; this is a no-op unless
    /// a host overrides it.
    fn set_reduce_motion(&self, _enabled: bool) {}
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

    pub(crate) fn bind_list(&self, list: NodeId, binds: &[(u32, u32)]) {
        self.0.bind_list(list, binds);
    }

    pub(crate) fn animations_finished(&self, finished: &[nlayer_core::animation::Finished]) {
        self.0.animations_finished(finished);
    }

    pub(crate) fn dispatch_back(&self) -> bool {
        self.0.dispatch_back()
    }

    pub(crate) fn set_reduce_motion(&self, enabled: bool) {
        self.0.set_reduce_motion(enabled);
    }

    pub(crate) fn next_timer(&self) -> Option<std::time::Duration> {
        None
    }

    pub(crate) fn run_due_timers(&self) {}
}
