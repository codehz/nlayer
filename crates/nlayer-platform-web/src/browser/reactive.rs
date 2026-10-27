//! The page's reactive graph: the same scheduler as QuickJS, with slot
//! values instead of QuickJS values.

use nlayer_core::reactive::{self as r, Equals, Host, Kind};

use super::{
    PropError, Runtime,
    values::{Args, Value},
};

#[derive(Clone, Copy)]
pub(super) struct JsHost(*mut Runtime);

impl JsHost {
    pub(super) fn new(runtime: *mut Runtime) -> Self {
        JsHost(runtime)
    }
}
pub(super) struct Data {
    pub(super) error: Option<Value>,
    /// The object `ErrorBoundary` provides, so a failed computation can
    /// find the boundary signal.
    pub(super) boundary: Value,
    pub(super) running: u32,
}

pub(super) type Graph = r::Graph<JsHost>;

pub(super) struct Frame {
    key: Value,
    value: Value,
}

// SAFETY: the graph is a field of `Runtime`, which stays put. Borrows are
// short and end before a call into the page, which may re-enter.
unsafe impl r::Host for JsHost {
    type Value = Value;
    type Func = Value;
    type Equals = Value;
    type Cleanup = Value;
    type Context = Frame;
    type Data = Data;

    unsafe fn graph<'a>(self) -> &'a mut Graph {
        // SAFETY: the caller holds no other reference. `Runtime` is not moved.
        unsafe { &mut (*self.0).graph }
    }

    fn empty() -> Value {
        Value::unit()
    }

    fn dup(self, v: &Value) -> Value {
        v.clone()
    }

    fn free(self, v: Value) {
        drop(v);
    }

    fn dup_func(self, f: &Value) -> Value {
        f.clone()
    }

    fn free_func(self, f: Value) {
        drop(f);
    }

    fn dup_equals(self, e: &Value) -> Value {
        e.clone()
    }

    fn free_equals(self, e: Value) {
        drop(e);
    }

    fn call(self, f: &Value, prev: Value) -> Option<Value> {
        unsafe { self.graph() }.data.running += 1;
        let args = match Value::array([prev]) {
            Ok(args) => args,
            Err(_) => {
                unsafe { self.graph() }.data.running -= 1;
                return None;
            }
        };
        let result = f.call(&Value::unit(), &args);
        unsafe { self.graph() }.data.running -= 1;
        match result {
            Ok(value) => Some(value),
            Err(_) => {
                catch(self, Value::last_error());
                None
            }
        }
    }

    fn same(self, a: &Value, b: &Value) -> bool {
        a.same(b)
    }

    fn truthy(self, v: &Value) -> bool {
        v.truthy()
    }

    fn custom_changed(self, e: &Value, prev: Value, next: &Value) -> bool {
        let args = match Value::array([prev, next.clone()]) {
            Ok(args) => args,
            Err(_) => return true,
        };
        match e.call(&Value::unit(), &args) {
            Ok(same) => !same.truthy(),
            Err(_) => true,
        }
    }

    fn run_cleanup(self, f: Value) {
        let args = Value::array([]).unwrap_or_else(|_| Value::unit());
        let _ = f.call(&Value::unit(), &args);
    }

    fn free_cleanup(self, f: Value) {
        drop(f);
    }

    fn runaway(self) {
        let message =
            Value::string("potential infinite loop: an effect keeps changing what it reads");
        if let Ok(message) = message {
            unsafe { self.graph() }.data.error = Some(message);
        }
    }
}

fn catch(host: JsHost, error: Value) {
    let key = unsafe { host.graph() }.data.boundary.clone();
    let mut frame = unsafe { host.graph() }.context();
    let mut signal = None;
    while let Some(next) = frame {
        if host.same(&next.data.key, &key) {
            signal = Some(next.data.value.clone());
            break;
        }
        frame = next.parent.clone();
    }
    let Some(signal) = signal else {
        unsafe { host.graph() }.data.error = Some(error);
        return;
    };
    let Ok(set) = signal.get("set") else {
        unsafe { host.graph() }.data.error = Some(error);
        return;
    };
    let Ok(list) = Value::array([error]) else {
        return;
    };
    let Ok(args) = Value::array([list]) else {
        return;
    };
    if set.call(&signal, &args).is_err() {
        unsafe { host.graph() }.data.error = Some(Value::unit());
    }
}

fn equals_of(options: &Value) -> Result<Equals<Value>, PropError> {
    if options.kind() != 7 {
        return Ok(Equals::Default);
    }
    let equals = options.get("equals")?;
    if equals.kind() == 6 {
        return Ok(Equals::Custom(equals));
    }
    if equals.kind() == 2 && !equals.bool() {
        return Ok(Equals::Never);
    }
    Ok(Equals::Default)
}

fn node_id(value: &Value) -> Result<u32, PropError> {
    value
        .number_value()
        .map(|n| n as u32)
        .ok_or_else(|| PropError("not a reactive node".into()))
}

impl Runtime {
    pub(super) fn reactive(&self, op: u32, args: &Args) -> Result<Value, PropError> {
        let host = self.host();
        let result = match op {
            100 => {
                let value = args.index(0)?;
                let equals = equals_of(&args.index(1)?)?;
                let id = unsafe { host.graph() }.signal(value, equals);
                Value::number(id as f64)
            }
            101 => {
                let id = node_id(&args.index(0)?)?;
                Ok(r::read(host, id))
            }
            102 => {
                let id = node_id(&args.index(0)?)?;
                let value = args.index(1)?;
                r::write(host, id, value);
                Ok(Value::unit())
            }
            103 => {
                let func = args.index(0)?;
                let equals = equals_of(&args.index(1)?)?;
                let id = r::memo(host, func, Value::unit(), equals, true);
                Value::number(id as f64)
            }
            104 => {
                let func = args.index(0)?;
                let kind = if args.index(1)?.truthy() {
                    Kind::UserEffect
                } else {
                    Kind::Effect
                };
                r::effect(host, kind, func, Value::unit());
                Ok(Value::unit())
            }
            105 => {
                let func = args.index(0)?;
                let (owner, result) = r::with_root(host, || {
                    let id = unsafe { host.graph() }.owner();
                    let args = Value::array([Value::number(id as f64)?])?;
                    func.call(&Value::unit(), &args)
                });
                if result.is_err() {
                    r::dispose_owner(host, owner);
                }
                result
            }
            106 => {
                let id = node_id(&args.index(0)?)?;
                r::dispose_owner(host, id);
                Ok(Value::unit())
            }
            107 => {
                let func = args.index(0)?;
                let none = Value::array([])?;
                r::batch(host, || func.call(&Value::unit(), &none))
            }
            108 => {
                let func = args.index(0)?;
                let none = Value::array([])?;
                r::untracked(host, || func.call(&Value::unit(), &none))
            }
            109 => {
                let func = args.index(0)?;
                unsafe { host.graph() }
                    .on_cleanup(func)
                    .map_err(|_| PropError("cleanup requires an owner".into()))?;
                Ok(Value::unit())
            }
            112 => {
                let key = args.index(0)?;
                let fallback = args.index(1)?;
                let mut frame = unsafe { host.graph() }.context();
                let mut found = None;
                while let Some(next) = frame {
                    if host.same(&next.data.key, &key) {
                        found = Some(next.data.value.clone());
                        break;
                    }
                    frame = next.parent.clone();
                }
                Ok(found.unwrap_or(fallback))
            }
            113 => {
                let key = args.index(0)?;
                let value = args.index(1)?;
                unsafe { host.graph() }
                    .provide(Frame { key, value })
                    .map_err(|_| PropError("context requires an owner".into()))?;
                Ok(Value::unit())
            }
            114 => {
                let id = node_id(&args.index(0)?)?;
                if let Some(held) = unsafe { host.graph() }.drop_handle(id) {
                    held.free(host);
                }
                Ok(Value::unit())
            }
            116 => Ok(unsafe { host.graph() }.data.boundary.clone()),
            _ => Err(PropError("unknown reactive operation".into())),
        };
        let error = unsafe { host.graph() }.data.error.take();
        let in_round = unsafe { host.graph() }.in_round();
        if let Some(error) = error {
            if in_round {
                unsafe { host.graph() }.data.error = Some(error);
                result
            } else {
                let message = error.as_string().unwrap_or_else(|| "nlayer".into());
                Err(PropError(message))
            }
        } else {
            result
        }
    }
}
