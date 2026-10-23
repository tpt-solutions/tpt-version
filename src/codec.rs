//! Canonical binary encoding shared by every stored object.
//!
//! Integers are fixed-width big-endian and variable-length fields carry a
//! `u64` length prefix, so each value has exactly one encoding.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::ObjectError;
use crate::object::ContentHash;

/// Builds an object's bytes, starting with its type tag.
pub(crate) struct Writer(Vec<u8>);

impl Writer {
    pub(crate) fn new(tag: u8) -> Self {
        Self(alloc::vec![tag])
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.0
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn i64(&mut self, value: i64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn count(&mut self, value: usize) {
        self.u64(value as u64);
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.count(value.len());
        self.0.extend_from_slice(value);
    }

    pub(crate) fn str(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    /// Writes a fixed-size field with no length prefix.
    pub(crate) fn fixed(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }

    pub(crate) fn opt_hash(&mut self, value: Option<&ContentHash>) {
        match value {
            Some(hash) => {
                self.u8(1);
                self.fixed(hash.as_bytes());
            }
            None => self.u8(0),
        }
    }
}

/// Reads an object's bytes back. Truncated input is an error, never a panic.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading `data`, returning its leading type tag.
    pub(crate) fn open(data: &'a [u8]) -> Result<(u8, Self), ObjectError> {
        let mut reader = Self { data, pos: 0 };
        let tag = reader.u8()?;
        Ok((tag, reader))
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], ObjectError> {
        let data: &'a [u8] = self.data;
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= data.len())
            .ok_or(ObjectError::Truncated)?;
        let slice = &data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ObjectError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ObjectError> {
        Ok(u64::from_be_bytes(self.fixed()?))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, ObjectError> {
        Ok(i64::from_be_bytes(self.fixed()?))
    }

    pub(crate) fn count(&mut self) -> Result<usize, ObjectError> {
        usize::try_from(self.u64()?).map_err(|_| ObjectError::Truncated)
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>, ObjectError> {
        let len = self.count()?;
        Ok(self.take(len)?.to_vec())
    }

    pub(crate) fn str(&mut self) -> Result<String, ObjectError> {
        String::from_utf8(self.bytes()?).map_err(|_| ObjectError::InvalidUtf8)
    }

    pub(crate) fn fixed<const N: usize>(&mut self) -> Result<[u8; N], ObjectError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn hash(&mut self) -> Result<ContentHash, ObjectError> {
        Ok(ContentHash::from_bytes(self.fixed()?))
    }

    pub(crate) fn opt_hash(&mut self) -> Result<Option<ContentHash>, ObjectError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.hash().map(Some),
            _ => Err(ObjectError::InvalidValue("option flag")),
        }
    }

    /// Fails if any bytes remain unread.
    pub(crate) fn finish(self) -> Result<(), ObjectError> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(ObjectError::TrailingBytes)
        }
    }
}
