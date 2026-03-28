//! Error types for shravan.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type alias for shravan operations.
pub type Result<T> = core::result::Result<T, ShravanError>;

/// Errors produced by shravan codec operations.
#[derive(Debug, Error, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ShravanError {
    /// The audio format is not supported.
    #[error("unsupported format")]
    UnsupportedFormat,

    /// The file header is invalid or corrupt.
    #[error("invalid header: {0}")]
    InvalidHeader(String),

    /// An error occurred during decoding.
    #[error("decode error: {0}")]
    DecodeError(String),

    /// An error occurred during encoding.
    #[error("encode error: {0}")]
    EncodeError(String),

    /// Unexpected end of input data.
    #[error("unexpected end of stream")]
    EndOfStream,

    /// The sample rate is invalid or unsupported.
    #[error("invalid sample rate: {0} Hz")]
    InvalidSampleRate(u32),

    /// The channel count is invalid or unsupported.
    #[error("invalid channel count: {0}")]
    InvalidChannels(u16),
}
