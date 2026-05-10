//! seL4 error types.
//!
//! The kernel returns error codes from system calls as a word-sized value.
//! This module defines the known error codes and a `Result` type alias.

use core::fmt;

/// seL4 system call error codes.
///
/// A return value of 0 indicates success. Non-zero values encode specific
/// error conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Error {
    /// Operation was successful.
    Success = 0,

    /// Invalid argument.
    InvalidArgument = 1,
    /// Invalid capability.
    InvalidCapability = 2,
    /// Illegal operation.
    IllegalOperation = 3,
    /// Range error (e.g., address out of bounds).
    RangeError = 4,
    /// Alignment error.
    AlignmentError = 5,
    /// Failed lookup (capability not found or depth mismatch).
    FailedLookup = 6,
    /// Truncated message.
    TruncatedMessage = 7,
    /// Delete in progress (object is being deleted).
    DeleteFirst = 8,
    /// Revoke in progress.
    RevokeFirst = 9,
    /// Not enough memory.
    NotEnoughMemory = 10,
}

impl Error {
    /// Convert a raw seL4 error word into an `Error`.
    pub fn from_word(word: usize) -> Self {
        match word {
            0 => Error::Success,
            1 => Error::InvalidArgument,
            2 => Error::InvalidCapability,
            3 => Error::IllegalOperation,
            4 => Error::RangeError,
            5 => Error::AlignmentError,
            6 => Error::FailedLookup,
            7 => Error::TruncatedMessage,
            8 => Error::DeleteFirst,
            9 => Error::RevokeFirst,
            10 => Error::NotEnoughMemory,
            _ => {
                // Unknown error code — preserve the raw value for debugging
                // but we can't represent it as a valid Error variant.
                // This should not happen in practice.
                Error::InvalidArgument
            }
        }
    }

    /// Check if this error indicates success.
    pub fn is_ok(self) -> bool {
        matches!(self, Error::Success)
    }

    /// Check if this error indicates failure.
    pub fn is_err(self) -> bool {
        !self.is_ok()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Success => write!(f, "success"),
            Error::InvalidArgument => write!(f, "invalid argument"),
            Error::InvalidCapability => write!(f, "invalid capability"),
            Error::IllegalOperation => write!(f, "illegal operation"),
            Error::RangeError => write!(f, "range error"),
            Error::AlignmentError => write!(f, "alignment error"),
            Error::FailedLookup => write!(f, "failed lookup"),
            Error::TruncatedMessage => write!(f, "truncated message"),
            Error::DeleteFirst => write!(f, "delete first"),
            Error::RevokeFirst => write!(f, "revoke first"),
            Error::NotEnoughMemory => write!(f, "not enough memory"),
        }
    }
}

/// seL4 result type alias.
///
/// Most seL4 operations return `usize` where `0` means success. This type
/// wraps that convention.
pub type Result<T> = core::result::Result<T, Error>;

/// Assert that an seL4 operation succeeded, returning the error code on
/// failure.
///
/// This is a convenience function for use with raw seL4 return codes.
#[inline]
pub fn check(raw: usize) -> core::result::Result<(), Error> {
    if raw == 0 {
        Ok(())
    } else {
        Err(Error::from_word(raw))
    }
}

/// Assert that an seL4 operation succeeded, panicking if it failed.
///
/// Use this for operations that should never fail (e.g., during
/// initialization).
#[inline]
pub fn assert_ok(raw: usize, context: &str) {
    if raw != 0 {
        panic!(
            "seL4 operation failed at '{}': {} (code {})",
            context,
            Error::from_word(raw),
            raw
        );
    }
}
