//! Structural diffs: which entities changed between two snapshots, and which
//! fields changed inside a modified entity.

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use crate::dag;
use crate::error::RepoError;
use crate::object::{ContentHash, EntityId, Tree};
use crate::store::{CasStore, load_entity, load_tree};
use crate::uir::UirNode;

/// One entity's change between two snapshots.
#[derive(Debug, Clone, PartialEq)]
pub enum DiffEntry {
    /// The entity exists only in the newer snapshot.
    Added {
        /// The entity's id.
        id: EntityId,
        /// The hash of its content in the newer snapshot.
        hash: ContentHash,
    },
    /// The entity exists only in the older snapshot.
    Removed {
        /// The entity's id.
        id: EntityId,
        /// The hash of its content in the older snapshot.
        hash: ContentHash,
    },
    /// The id survived but its content changed. The field-level detail is in `diff`.
    Modified {
        /// The entity's id.
        id: EntityId,
        /// The hash of its content in the older snapshot.
        old_hash: ContentHash,
        /// The hash of its content in the newer snapshot.
        new_hash: ContentHash,
        /// The changed fields, in path order.
        diff: EntityDiff,
    },
}

/// The field-level changes inside one modified entity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityDiff {
    /// One entry per changed field path.
    pub field_changes: Vec<FieldChange>,
}

/// A change at one field path. `None` on one side means the field was added or removed.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    /// The path from the entity root to the field, one name per level.
    pub field_path: Vec<String>,
    /// The value in the older snapshot, or `None` if the field was absent there.
    pub old_value: Option<UirNode>,
    /// The value in the newer snapshot, or `None` if the field was removed.
    pub new_value: Option<UirNode>,
}

/// Compares two trees by entity id. Unchanged entities are skipped.
pub fn diff_trees<S: CasStore>(
    store: &S,
    old: &Tree,
    new: &Tree,
) -> Result<Vec<DiffEntry>, RepoError> {
    let ids: BTreeSet<&EntityId> = old.entries.keys().chain(new.entries.keys()).collect();
    let mut entries = Vec::new();
    for id in ids {
        match (old.entries.get(id), new.entries.get(id)) {
            (None, Some(hash)) => entries.push(DiffEntry::Added {
                id: *id,
                hash: *hash,
            }),
            (Some(hash), None) => entries.push(DiffEntry::Removed {
                id: *id,
                hash: *hash,
            }),
            (Some(old_hash), Some(new_hash)) if old_hash != new_hash => {
                let old_node = load_entity(store, old_hash)?;
                let new_node = load_entity(store, new_hash)?;
                entries.push(DiffEntry::Modified {
                    id: *id,
                    old_hash: *old_hash,
                    new_hash: *new_hash,
                    diff: diff_entities(&old_node, &new_node),
                });
            }
            _ => {}
        }
    }
    Ok(entries)
}

/// Compares the trees of two commits.
pub fn diff_commits<S: CasStore>(
    store: &S,
    old: &ContentHash,
    new: &ContentHash,
) -> Result<Vec<DiffEntry>, RepoError> {
    let old_tree = load_tree(store, &dag::commit_tree(store, old)?)?;
    let new_tree = load_tree(store, &dag::commit_tree(store, new)?)?;
    diff_trees(store, &old_tree, &new_tree)
}

/// Field-level diff of two entity values.
///
/// Maps are compared key by key, so moving one vertex coordinate is one
/// change, not a delete plus an insert. Lists and scalars are compared as
/// whole values.
pub fn diff_entities(old: &UirNode, new: &UirNode) -> EntityDiff {
    let mut field_changes = Vec::new();
    diff_nodes(&mut Vec::new(), Some(old), Some(new), &mut field_changes);
    EntityDiff { field_changes }
}

fn diff_nodes(
    path: &mut Vec<String>,
    old: Option<&UirNode>,
    new: Option<&UirNode>,
    changes: &mut Vec<FieldChange>,
) {
    if old == new {
        return;
    }
    if let (Some(UirNode::Map(old_map)), Some(UirNode::Map(new_map))) = (old, new) {
        let keys: BTreeSet<&String> = old_map.keys().chain(new_map.keys()).collect();
        for key in keys {
            path.push(key.clone());
            diff_nodes(path, old_map.get(key), new_map.get(key), changes);
            path.pop();
        }
        return;
    }
    changes.push(FieldChange {
        field_path: path.clone(),
        old_value: old.cloned(),
        new_value: new.cloned(),
    });
}
