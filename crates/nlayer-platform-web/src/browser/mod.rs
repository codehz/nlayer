//! Page-side bridge. The app's modules run in the browser; this module
//! holds the reactive graph and the node table and links no QuickJS.
#![allow(unreachable_pub)]
mod dom;
mod protocol;
mod reactive;
mod values;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use nlayer_core::{EventKind, NodeId, Op};
use nlayer_engine::{self, Bridge, EventPayload, Os};
use values::Value;

use crate::browser::reactive::{Data, Graph, JsHost};

/// A failed page call.
pub(super) struct PropError(pub String);

impl PropError {
    fn failed() -> Self {
        PropError("browser value operation failed".into())
    }
}

impl From<&str> for PropError {
    fn from(message: &str) -> Self {
        PropError(message.into())
    }
}

impl From<String> for PropError {
    fn from(message: String) -> Self {
        PropError(message)
    }
}

pub(super) struct Runtime {
    graph: Graph,
    dom: dom::Dom,
    event: RefCell<Option<Value>>,
    os: Cell<Os>,
}

impl Runtime {
    fn host(&self) -> JsHost {
        JsHost::new(std::ptr::from_ref(self) as *mut Runtime)
    }

    fn dispatch(&self, op: u32, args: &values::Args) -> Result<Value, PropError> {
        if op == 400 {
            *self.event.borrow_mut() = Some(args.index(0)?);
            return Ok(Value::unit());
        }
        if op >= 300 {
            self.dom_call(op, args)
        } else {
            self.reactive(op, args)
        }
    }
}

impl Bridge for Runtime {
    fn take_ops(&self, out: &mut Vec<Op>) {
        self.dom.take(out);
    }

    fn dispatch_event(&self, id: NodeId, event: EventKind, payload: &EventPayload) {
        let Some(callback) = self.event.borrow().clone() else {
            return;
        };
        let Ok(name) = Value::string(event.prop_name()) else {
            return;
        };
        let payload = match payload {
            EventPayload::None => Value::null(),
            EventPayload::Text(text) => Value::string(text).unwrap_or_else(|_| Value::null()),
        };
        let Ok(id) = Value::number(id as f64) else {
            return;
        };
        let Ok(args) = Value::array([id, name, payload]) else {
            return;
        };
        let _ = callback.call(&Value::unit(), &args);
    }

    fn set_os(&self, os: Os) {
        self.os.set(os);
    }
}

thread_local! {
    static RT: RefCell<Option<Rc<Runtime>>> = const { RefCell::new(None) };
}

fn current() -> Option<Rc<Runtime>> {
    RT.with(|slot| slot.borrow().clone())
}

/// Starts one graph. The page disposes the previous app first.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_browser_initialize() {
    let Ok(boundary) = Value::object() else {
        return;
    };
    let runtime = Rc::new(Runtime {
        graph: Graph::new(Data {
            error: None,
            boundary,
            running: 0,
        }),
        dom: dom::Dom::new(),
        event: RefCell::new(None),
        os: Cell::new(Os::Web),
    });
    if nlayer_engine::register(Rc::clone(&runtime) as Rc<dyn Bridge>).is_err() {
        return;
    }
    RT.with(|slot| *slot.borrow_mut() = Some(runtime));
}

/// Drops the graph after the page has stopped the engine.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_browser_dispose() {
    RT.with(|slot| *slot.borrow_mut() = None);
    nlayer_engine::clear_registration();
}

/// One call from the page. Arguments are an array slot. The result is an
/// owned slot, or [`values::EXCEPTION`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_browser_call(op: u32, args: u32) -> u32 {
    let Some(runtime) = current() else {
        return values::EXCEPTION;
    };
    match runtime.dispatch(op, &values::Args(args)) {
        Ok(value) => value.into_raw(),
        Err(error) => {
            log::error!("{}", error.0);
            values::EXCEPTION
        }
    }
}
