//! Error types.

use displaydoc::Display;
use thiserror::Error;

/// Result of handling flatbuffer data.
pub type Result<T> = ::core::result::Result<T, Error>;

#[derive(Debug, Display, Error)]
/// Message parsing structural error.
pub enum Error {
    /// Tried to access location outside of message buffer.
    OutOfBounds,
    /// Buffer cannot possibly contain a vtable (too small).
    MissingVTable,
    /// Integer types overflowed while doing offset calculations.
    ///
    /// Note: This error only occurs on 32-bit and large inputs, or due to
    ///       malicious inputs.
    IntegerOverflow,
}
