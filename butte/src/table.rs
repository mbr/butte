/*
 * Copyright 2018 Google Inc. All rights reserved.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::{
    buf_ptr::BufPtr,
    error::{Error, Result},
    follow::Follow,
    primitives::*,
    vtable::VTable,
};
use std::convert::TryFrom;
use std::mem;

/// Read-wrapper for table values.
#[derive(Debug)]
pub struct Table<'a> {
    start: BufPtr<'a>,
    // FIXME: We duplicate the slice here, requiring an extra 8 bytes of storage.
    vtable: VTable<'a>,
}

impl<'a> Table<'a> {
    /// Create a new table reader over `ptr`.
    ///
    /// Returns an error if the vtable offset cannot possibly fit into the
    /// remaining bytes or if it points out of bounds.
    #[inline]
    pub fn new(ptr: BufPtr<'a>) -> Result<Self> {
        let tbl = ptr.as_slice();

        if tbl.len() < mem::size_of::<SOffsetT>() {
            return Err(Error::MissingVTable);
        }

        // TODO: Change VTable to use `BufPtr` as well.
        let vtable = VTable::init(ptr.buf, ptr.loc as usize);

        Ok(Table { start: ptr, vtable })
    }

    /// Create new table reader from buffer and location.
    ///
    /// Convenience function for the most common case. See `new` for details
    /// on errors returned.
    ///
    /// # Panics
    ///
    /// On 128 bit machines, panics if `loc` is larger than `i64::MAX`, which
    /// should never happen with data deserialized from flatbuffers.
    #[inline]
    pub fn from_buf_loc(buf: &'a [u8], loc: usize) -> Result<Self> {
        Table::new(BufPtr::new(
            buf,
            i64::try_from(loc).expect("Could not convert `loc` to `i64` (too large)"),
        )?)
    }

    #[inline]
    pub fn get<T: Follow<'a> + 'a>(&self, slot_byte_loc: VOffsetT) -> Result<Option<T::Inner>> {
        let o = self.vtable().get(slot_byte_loc) as usize;
        if o == 0 {
            return Ok(None);
        }
        Ok(Some(<T>::follow(
            self.start.buf,
            self.start.loc as usize + o,
        )))
    }

    #[inline]
    pub fn vtable(&self) -> &VTable<'a> {
        &self.vtable
    }
}

impl<'a> Follow<'a> for Table<'a> {
    type Inner = Table<'a>;
    #[inline]
    fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Table::from_buf_loc(buf, loc)
            .expect("FIXME: Follow currently has no way of reporting out of bounds errors.")
    }
}

#[inline]
pub fn get_root<'a, T: Follow<'a> + 'a>(data: &'a [u8]) -> T::Inner {
    <ForwardsUOffset<T>>::follow(data, 0)
}
#[inline]
pub fn get_size_prefixed_root<'a, T: Follow<'a> + 'a>(data: &'a [u8]) -> T::Inner {
    <SkipSizePrefix<ForwardsUOffset<T>>>::follow(data, 0)
}
#[inline]
pub fn buffer_has_identifier(data: &[u8], ident: &str, size_prefixed: bool) -> bool {
    assert_eq!(ident.len(), FILE_IDENTIFIER_LENGTH);

    let got = if size_prefixed {
        <SkipSizePrefix<SkipRootOffset<FileIdentifier>>>::follow(data, 0)
    } else {
        <SkipRootOffset<FileIdentifier>>::follow(data, 0)
    };

    ident.as_bytes() == got
}
