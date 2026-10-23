//! The entity contract: anything versioned by tpt-version.

use crate::error::ObjectError;
use crate::object::EntityId;
use crate::uir::UirNode;

/// A versioned entity, such as a CAD vertex or a spreadsheet cell.
///
/// This is a local stand-in for the `tpt-model` entity trait, which has no
/// published code yet. Keep the method names aligned with it when it lands.
pub trait Entity: Sized {
    /// The stable identity that survives edits.
    fn id(&self) -> EntityId;

    /// The canonical structured form that is hashed and stored.
    fn to_uir(&self) -> UirNode;

    /// Rebuilds the entity from its stored form.
    fn from_uir(node: &UirNode) -> Result<Self, ObjectError>;
}
