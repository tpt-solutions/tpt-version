//! Constraints that a repository's snapshots must satisfy before they are
//! committed.
//!
//! A [`Validator`] sees every entity in the snapshot at once, so it can check
//! rules that span entities, such as a reference that must name an existing
//! entity. A commit is refused while any validator reports a problem, the same
//! way it is refused while a conflict is pending. Nothing is repaired silently:
//! the caller fixes the entities and commits again.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::object::EntityId;
use crate::uir::UirNode;

/// A rule over the whole snapshot.
///
/// `entities` holds every entity in the snapshot, decoded to its UIR form and
/// keyed by id. Entities in a pending conflict are left out, because a commit
/// is refused while a conflict is pending anyway.
pub trait Validator {
    /// Returns one message per broken rule. An empty list means the snapshot
    /// passes this rule.
    fn check(&self, entities: &BTreeMap<EntityId, UirNode>) -> Vec<String>;
}
