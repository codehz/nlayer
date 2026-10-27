//! Owned handles for values that live in the page.
//!
//! A slot id is only meaningful to this module's page. Dropping a [`Value`]
//! releases one reference. Slots 0–3 are `undefined`, `null`, `false` and
//! `true`; releasing them does nothing.

use super::PropError;

pub(super) const EXCEPTION: u32 = u32::MAX;

#[link(wasm_import_module = "nlayer_browser")]
unsafe extern "C" {
    fn value(op: u32, a: u32, b: u32, c: u32, number: f64) -> f64;
}

pub(super) fn invoke(op: u32, a: u32, b: u32, c: u32, number: f64) -> f64 {
    // SAFETY: the page installed this import. Pointers name live bytes for
    // this synchronous call only.
    unsafe { value(op, a, b, c, number) }
}

fn owned(result: f64) -> Result<u32, PropError> {
    if result.is_nan() || result == EXCEPTION as f64 {
        return Err(PropError::failed());
    }
    Ok(result as u32)
}

/// A reference to a page value. Clone retains; drop releases.
#[derive(Debug)]
pub(super) struct Value(pub u32);

impl Clone for Value {
    fn clone(&self) -> Self {
        invoke(16, self.0, 0, 0, 0.0);
        Value(self.0)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        invoke(17, self.0, 0, 0, 0.0);
    }
}

impl Value {
    pub(super) fn into_raw(self) -> u32 {
        let id = self.0;
        std::mem::forget(self);
        id
    }

    pub(super) fn unit() -> Self {
        Value(0)
    }

    pub(super) fn null() -> Self {
        Value(1)
    }

    pub(super) fn number(n: f64) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(18, 0, 0, 0, n))?))
    }

    pub(super) fn string(s: &str) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(
            19,
            0,
            s.as_ptr() as u32,
            s.len() as u32,
            0.0,
        ))?))
    }

    pub(super) fn array(items: impl IntoIterator<Item = Self>) -> Result<Self, PropError> {
        let array = Value(owned(invoke(21, 0, 0, 0, 0.0))?);
        for (i, item) in items.into_iter().enumerate() {
            invoke(22, array.0, i as u32, item.into_raw(), 0.0);
        }
        Ok(array)
    }

    pub(super) fn object() -> Result<Self, PropError> {
        Ok(Value(owned(invoke(23, 0, 0, 0, 0.0))?))
    }

    pub(super) fn kind(&self) -> u32 {
        invoke(0, self.0, 0, 0, 0.0) as u32
    }

    pub(super) fn bool(&self) -> bool {
        invoke(1, self.0, 0, 0, 0.0) == 1.0
    }

    pub(super) fn number_value(&self) -> Option<f64> {
        let n = invoke(2, self.0, 0, 0, 0.0);
        (!n.is_nan()).then_some(n)
    }

    pub(super) fn as_string(&self) -> Option<String> {
        let len = invoke(3, self.0, 0, 0, 0.0);
        if len.is_nan() {
            return None;
        }
        let len = len as usize;
        let mut buf = vec![0u8; len];
        invoke(4, self.0, buf.as_mut_ptr() as u32, len as u32, 0.0);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    pub(super) fn to_string(&self) -> Result<String, PropError> {
        Value(owned(invoke(43, self.0, 0, 0, 0.0))?)
            .as_string()
            .ok_or_else(PropError::failed)
    }

    pub(super) fn length(&self) -> Option<usize> {
        let n = invoke(7, self.0, 0, 0, 0.0);
        (!n.is_nan()).then_some(n as usize)
    }

    pub(super) fn index(&self, i: usize) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(8, self.0, i as u32, 0, 0.0))?))
    }

    pub(super) fn get(&self, key: &str) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(
            9,
            self.0,
            key.as_ptr() as u32,
            key.len() as u32,
            0.0,
        ))?))
    }

    pub(super) fn keys(&self) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(10, self.0, 0, 0, 0.0))?))
    }

    pub(super) fn node(&self) -> Option<u32> {
        let id = invoke(14, self.0, 0, 0, 0.0) as u32;
        (id != 0).then_some(id)
    }

    pub(super) fn call(&self, this: &Self, args: &Self) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(26, self.0, this.0, args.0, 0.0))?))
    }

    /// The message of the last failed page call.
    pub(super) fn last_error() -> Self {
        Value(owned(invoke(38, 0, 0, 0, 0.0)).unwrap_or(0))
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        invoke(32, self.0, other.0, 0, 0.0) != 0.0
    }

    pub(super) fn truthy(&self) -> bool {
        match self.kind() {
            0 | 1 => false,
            2 => self.bool(),
            3 => self.number_value().is_some_and(|n| n != 0.0 && !n.is_nan()),
            4 => self.as_string().is_some_and(|s| !s.is_empty()),
            _ => true,
        }
    }

    /// The page makes the node object for `id` (`text`: a text node).
    pub(super) fn node_object(id: u32, text: bool) -> Result<Self, PropError> {
        Ok(Value(owned(invoke(39, id, u32::from(text), 0, 0.0))?))
    }
}

/// Arguments of one [`super::nlayer_browser_call`], borrowed from the page.
pub(super) struct Args(pub u32);

impl Args {
    pub(super) fn index(&self, i: usize) -> Result<Value, PropError> {
        Ok(Value(owned(invoke(8, self.0, i as u32, 0, 0.0))?))
    }
}
