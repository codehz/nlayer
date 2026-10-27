use rquickjs::{CaughtError, Ctx};

/// A JS exception or engine error, rendered to a message with stack.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct JsError {
    /// Message, including the JS stack when available.
    pub message: String,
}

impl JsError {
    pub(crate) fn caught(err: CaughtError<'_>) -> Self {
        Self {
            message: err.to_string(),
        }
    }

    pub(crate) fn new(message: String) -> Self {
        Self { message }
    }

    pub(crate) fn from_engine(cx: &Ctx<'_>, err: rquickjs::Error) -> Self {
        Self::caught(CaughtError::from_error(cx, err))
    }
}

impl From<rquickjs::Error> for JsError {
    fn from(err: rquickjs::Error) -> Self {
        Self::new(err.to_string())
    }
}
