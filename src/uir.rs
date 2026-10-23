//! Structured values carried by entities, and their canonical encoding.
//!
//! `tpt-uir-core` models IR operations and attributes rather than generic value
//! trees, so tpt-version defines the small value tree it needs here.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::codec::{Reader, Writer};
use crate::error::ObjectError;

/// Deepest nesting accepted when decoding, so hostile input cannot exhaust the stack.
const MAX_DEPTH: usize = 128;

/// A leaf value.
///
/// Floats compare by IEEE equality, so a `NaN` field never equals itself and
/// will surface as a conflict in a merge. Floats hash by bit pattern, so `0.0`
/// and `-0.0` are different content.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// No value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// A 64-bit float.
    Float(f64),
    /// UTF-8 text.
    Text(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
}

/// A structured value: a scalar, a list, or a map from field name to value.
#[derive(Debug, Clone, PartialEq)]
pub enum UirNode {
    /// A leaf value.
    Scalar(Scalar),
    /// An ordered sequence of values. Merged as one whole value.
    List(Vec<UirNode>),
    /// Named fields. Merged field by field.
    Map(BTreeMap<String, UirNode>),
}

impl From<bool> for UirNode {
    fn from(value: bool) -> Self {
        Self::Scalar(Scalar::Bool(value))
    }
}

impl From<i64> for UirNode {
    fn from(value: i64) -> Self {
        Self::Scalar(Scalar::Int(value))
    }
}

impl From<f64> for UirNode {
    fn from(value: f64) -> Self {
        Self::Scalar(Scalar::Float(value))
    }
}

impl From<&str> for UirNode {
    fn from(value: &str) -> Self {
        Self::Scalar(Scalar::Text(value.into()))
    }
}

impl UirNode {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Self::Scalar(scalar) => {
                w.u8(0);
                encode_scalar(scalar, w);
            }
            Self::List(items) => {
                w.u8(1);
                w.count(items.len());
                for item in items {
                    item.encode(w);
                }
            }
            Self::Map(entries) => {
                w.u8(2);
                w.count(entries.len());
                for (key, value) in entries {
                    w.str(key);
                    value.encode(w);
                }
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Self, ObjectError> {
        Self::decode_at(r, 0)
    }

    fn decode_at(r: &mut Reader<'_>, depth: usize) -> Result<Self, ObjectError> {
        if depth > MAX_DEPTH {
            return Err(ObjectError::InvalidValue("nesting depth"));
        }
        match r.u8()? {
            0 => Ok(Self::Scalar(decode_scalar(r)?)),
            1 => {
                let count = r.count()?;
                let mut items = Vec::new();
                for _ in 0..count {
                    items.push(Self::decode_at(r, depth + 1)?);
                }
                Ok(Self::List(items))
            }
            2 => {
                let count = r.count()?;
                let mut entries = BTreeMap::new();
                for _ in 0..count {
                    let key = r.str()?;
                    let value = Self::decode_at(r, depth + 1)?;
                    if entries.insert(key, value).is_some() {
                        return Err(ObjectError::InvalidValue("duplicate map key"));
                    }
                }
                Ok(Self::Map(entries))
            }
            _ => Err(ObjectError::InvalidValue("node kind")),
        }
    }
}

/// Writes an optional node with a presence flag.
pub(crate) fn encode_opt(w: &mut Writer, node: Option<&UirNode>) {
    match node {
        Some(node) => {
            w.u8(1);
            node.encode(w);
        }
        None => w.u8(0),
    }
}

/// Reads an optional node written by [`encode_opt`].
pub(crate) fn decode_opt(r: &mut Reader<'_>) -> Result<Option<UirNode>, ObjectError> {
    match r.u8()? {
        0 => Ok(None),
        1 => UirNode::decode(r).map(Some),
        _ => Err(ObjectError::InvalidValue("option flag")),
    }
}

fn encode_scalar(scalar: &Scalar, w: &mut Writer) {
    match scalar {
        Scalar::Null => w.u8(0),
        Scalar::Bool(value) => {
            w.u8(1);
            w.u8(u8::from(*value));
        }
        Scalar::Int(value) => {
            w.u8(2);
            w.i64(*value);
        }
        Scalar::Float(value) => {
            w.u8(3);
            w.u64(value.to_bits());
        }
        Scalar::Text(value) => {
            w.u8(4);
            w.str(value);
        }
        Scalar::Bytes(value) => {
            w.u8(5);
            w.bytes(value);
        }
    }
}

fn decode_scalar(r: &mut Reader<'_>) -> Result<Scalar, ObjectError> {
    match r.u8()? {
        0 => Ok(Scalar::Null),
        1 => match r.u8()? {
            0 => Ok(Scalar::Bool(false)),
            1 => Ok(Scalar::Bool(true)),
            _ => Err(ObjectError::InvalidValue("bool")),
        },
        2 => Ok(Scalar::Int(r.i64()?)),
        3 => Ok(Scalar::Float(f64::from_bits(r.u64()?))),
        4 => Ok(Scalar::Text(r.str()?)),
        5 => Ok(Scalar::Bytes(r.bytes()?)),
        _ => Err(ObjectError::InvalidValue("scalar kind")),
    }
}
