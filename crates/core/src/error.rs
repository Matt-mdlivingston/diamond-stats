//! Errors raised while parsing levels, innings, and API payloads.

use thiserror::Error;

/// A failure to interpret a baseball value or an API payload.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// The caller named a level this crate does not know.
    #[error("unrecognized level '{0}'")]
    UnknownLevel(String),
    /// Innings were not in baseball notation (`whole.0`, `whole.1`, or `whole.2`).
    #[error("invalid innings pitched '{0}'")]
    InvalidInnings(String),
    /// A stat row was missing an identity field or contradicted its context.
    #[error("{0}")]
    Message(String),
}
