//! Pointer-Offset structure ("obese pointers").

use crate::error::{Error, Result};
use std::{fmt, u64};

/// An pointer-style offset into a buffer.
///
/// `BufPtr` captures a read-only slice along with an offset into a single
/// struct that can be passed around. Although offsets are limited to 32 bits
/// in messages, multiple offsets can be aggregated inside a `BufPtr` over
/// the course of many indirections, for this reason they are stored as 64
/// bit integers internally.
///
/// Note that buffers larger than 4 GB will be problematic on 32 bit machines,
/// but this is no concern of the the `BufPtr`, as the memory will no be
/// addressable anyway.
///
/// Will never panic, but returns errors on illegal/out-of-bounds
/// operations instead.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct BufPtr<'a> {
    /// Underlying buffer.
    pub buf: &'a [u8],

    /// Offset from start of `buf`.
    ///
    /// This is always less or equal than the length of `buf` and always >= 0.
    pub loc: i64,
}

impl<'a> BufPtr<'a> {
    /// Create a new offset-into-buffer pointer.
    ///
    /// Returns `OutOfBounds` if `loc` is greater than the length `buf` or <0.
    #[inline]
    pub fn new(buf: &'a [u8], loc: i64) -> Result<Self> {
        // This will be problematic on 128 bit architectures only on really
        // large inputs.
        debug_assert!((buf.len() as u128) < (u64::MAX as u128));

        if loc < 0 || loc as u64 > buf.len() as u64 {
            return Err(Error::OutOfBounds);
        }

        Ok(BufPtr { buf, loc })
    }

    /// Return a slice with the offset applied to the original buffer.
    #[inline]
    pub fn as_slice(&self) -> &'a [u8] {
        debug_assert!(self.loc > 0);

        &self.buf[self.loc as usize..]
    }

    /// Create a new `BufPtr` on the same slice with an offset applied.
    ///
    /// Will return an `OutOfBounds` error if `offset` results in a position
    /// outside of the buffer, or `IntegerOverflow` if the resulting new
    /// location does not fit into a signed 64 bit integer.
    #[inline]
    pub fn with_offset(&self, offset: i32) -> Result<Self> {
        // Note that this handles positive as well as negative overflows.
        let (new_loc, overflow) = self.loc.overflowing_add(offset as i64);

        if overflow {
            return Err(Error::IntegerOverflow);
        }

        // `new` will handle the out-of-bounds check the end for us.
        BufPtr::new(self.buf, new_loc)
    }
}

impl<'a> fmt::Debug for BufPtr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BufPtr<{:x}+{}>", self.buf.as_ptr() as usize, self.loc)
    }
}
