//! The entity contract: anything versioned by tpt-version.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::ObjectError;
use crate::object::EntityId;
use crate::uir::{Scalar, UirNode};

/// A versioned entity, such as a CAD vertex or a spreadsheet cell.
///
/// This is a local stand-in for the `tpt-model` entity trait, which has no
/// published code yet. Keep the method names aligned with it when it lands.
///
/// Implement it by hand, or derive it with `#[derive(Entity)]` when the `derive`
/// feature is enabled.
pub trait Entity: Sized {
    /// The stable identity that survives edits.
    fn id(&self) -> EntityId;

    /// The canonical structured form that is hashed and stored.
    fn to_uir(&self) -> UirNode;

    /// Rebuilds the entity from its stored form.
    fn from_uir(node: &UirNode) -> Result<Self, ObjectError>;
}

/// A field type that can be stored inside an entity's [`UirNode`] form.
///
/// `#[derive(Entity)]` uses this for every field. Implement it for your own
/// types to store them directly as fields.
pub trait UirValue: Sized {
    /// The stored form of this value.
    fn to_uir_value(&self) -> UirNode;

    /// Rebuilds the value from its stored form.
    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError>;
}

fn invalid(what: &'static str) -> ObjectError {
    ObjectError::InvalidValue(what)
}

impl UirValue for bool {
    fn to_uir_value(&self) -> UirNode {
        UirNode::Scalar(Scalar::Bool(*self))
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Bool(value)) => Ok(*value),
            _ => Err(invalid("bool")),
        }
    }
}

impl UirValue for i64 {
    fn to_uir_value(&self) -> UirNode {
        UirNode::Scalar(Scalar::Int(*self))
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Int(value)) => Ok(*value),
            _ => Err(invalid("integer")),
        }
    }
}

impl UirValue for i32 {
    fn to_uir_value(&self) -> UirNode {
        i64::from(*self).to_uir_value()
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        Self::try_from(i64::from_uir_value(node)?).map_err(|_| invalid("i32 range"))
    }
}

impl UirValue for f64 {
    fn to_uir_value(&self) -> UirNode {
        UirNode::Scalar(Scalar::Float(*self))
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Float(value)) => Ok(*value),
            _ => Err(invalid("float")),
        }
    }
}

impl UirValue for String {
    fn to_uir_value(&self) -> UirNode {
        UirNode::Scalar(Scalar::Text(self.clone()))
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Text(text)) => Ok(text.clone()),
            _ => Err(invalid("text")),
        }
    }
}

impl UirValue for EntityId {
    fn to_uir_value(&self) -> UirNode {
        UirNode::Scalar(Scalar::Bytes(self.as_bytes().to_vec()))
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Bytes(bytes)) => <[u8; 16]>::try_from(bytes.as_slice())
                .map(EntityId::from_bytes)
                .map_err(|_| invalid("entity id length")),
            _ => Err(invalid("entity id")),
        }
    }
}

/// `None` is stored as `Null`, so a missing optional field is not an error.
impl<T: UirValue> UirValue for Option<T> {
    fn to_uir_value(&self) -> UirNode {
        match self {
            Some(value) => value.to_uir_value(),
            None => UirNode::Scalar(Scalar::Null),
        }
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::Scalar(Scalar::Null) => Ok(None),
            other => T::from_uir_value(other).map(Some),
        }
    }
}

impl<T: UirValue> UirValue for Vec<T> {
    fn to_uir_value(&self) -> UirNode {
        UirNode::List(self.iter().map(UirValue::to_uir_value).collect())
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        match node {
            UirNode::List(items) => items.iter().map(T::from_uir_value).collect(),
            _ => Err(invalid("list")),
        }
    }
}

impl<T: UirValue, const N: usize> UirValue for [T; N] {
    fn to_uir_value(&self) -> UirNode {
        UirNode::List(self.iter().map(UirValue::to_uir_value).collect())
    }

    fn from_uir_value(node: &UirNode) -> Result<Self, ObjectError> {
        let items: Vec<T> = Vec::from_uir_value(node)?;
        <[T; N]>::try_from(items).map_err(|_| invalid("array length"))
    }
}

/// Builds a map node from named fields. Used by `#[derive(Entity)]`.
#[doc(hidden)]
pub fn __map<const N: usize>(fields: [(&'static str, UirNode); N]) -> UirNode {
    UirNode::Map(
        fields
            .into_iter()
            .map(|(name, value)| (String::from(name), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// Looks up a named field of a map node. Used by `#[derive(Entity)]`.
#[doc(hidden)]
pub fn __field<'a>(node: &'a UirNode, name: &'static str) -> Result<&'a UirNode, ObjectError> {
    match node {
        UirNode::Map(fields) => fields.get(name).ok_or(invalid(name)),
        _ => Err(invalid("entity fields")),
    }
}
