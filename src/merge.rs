//! Three-way merge of trees, with field-level merging inside each entity.
//!
//! Nothing is resolved silently. An entity changed on both sides merges only
//! where the fields do not overlap; anything else becomes a stored
//! [`Conflict`] for the application to resolve.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::RepoError;
use crate::object::{Conflict, ContentHash, EntityId, FieldConflict, Object, Tree};
use crate::store::{CasStore, load_entity, load_tree, store_object};
use crate::uir::UirNode;

/// The outcome of a merge. `merged_tree` may reference conflict objects, which
/// are also listed in `conflicts`.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeResult {
    pub merged_tree: ContentHash,
    pub conflicts: Vec<Conflict>,
}

/// How the application resolves a conflicted entity.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Keep our version. Removes the entity if we deleted it.
    Ours,
    /// Keep their version. Removes the entity if they deleted it.
    Theirs,
    /// Use a value the application built, for example from a merge UI.
    Custom(UirNode),
}

/// Merges `ours` and `theirs` against `base`, given as tree hashes.
///
/// Per entity: a change on one side only takes that side. Identical changes
/// collapse. Changes on both sides merge field by field. Delete against
/// modify, or overlapping field edits, become conflicts.
pub fn merge_trees<S: CasStore>(
    store: &mut S,
    base: ContentHash,
    ours: ContentHash,
    theirs: ContentHash,
) -> Result<MergeResult, RepoError> {
    let base = load_tree(store, &base)?;
    let ours = load_tree(store, &ours)?;
    let theirs = load_tree(store, &theirs)?;

    let ids: BTreeSet<EntityId> = base
        .entries
        .keys()
        .chain(ours.entries.keys())
        .chain(theirs.entries.keys())
        .copied()
        .collect();

    let mut merged = Tree::default();
    let mut conflicts = Vec::new();
    for id in ids {
        let base_hash = base.entries.get(&id).copied();
        let ours_hash = ours.entries.get(&id).copied();
        let theirs_hash = theirs.entries.get(&id).copied();
        let outcome = if ours_hash == theirs_hash {
            ours_hash
        } else if base_hash == ours_hash {
            theirs_hash
        } else if base_hash == theirs_hash {
            ours_hash
        } else {
            merge_entity(store, id, base_hash, ours_hash, theirs_hash, &mut conflicts)?
        };
        if let Some(hash) = outcome {
            merged.entries.insert(id, hash);
        }
    }

    let merged_tree = store_object(store, &Object::Tree(merged))?;
    Ok(MergeResult {
        merged_tree,
        conflicts,
    })
}

fn merge_entity<S: CasStore>(
    store: &mut S,
    entity: EntityId,
    base: Option<ContentHash>,
    ours: Option<ContentHash>,
    theirs: Option<ContentHash>,
    conflicts: &mut Vec<Conflict>,
) -> Result<Option<ContentHash>, RepoError> {
    let (Some(our_hash), Some(their_hash)) = (ours, theirs) else {
        // One side deleted the entity while the other changed it.
        let whole = FieldConflict {
            path: Vec::new(),
            base: load_optional(store, base)?,
            ours: load_optional(store, ours)?,
            theirs: load_optional(store, theirs)?,
        };
        let hash = record_conflict(store, entity, base, ours, theirs, vec![whole], conflicts)?;
        return Ok(Some(hash));
    };

    let base_node = load_optional(store, base)?;
    let our_node = load_entity(store, &our_hash)?;
    let their_node = load_entity(store, &their_hash)?;
    let mut fields = Vec::new();
    let merged = merge_node(
        &mut Vec::new(),
        base_node.as_ref(),
        Some(&our_node),
        Some(&their_node),
        &mut fields,
    );
    match merged {
        Some(node) if fields.is_empty() => Ok(Some(store_object(store, &Object::Entity(node))?)),
        _ => {
            let hash = record_conflict(store, entity, base, ours, theirs, fields, conflicts)?;
            Ok(Some(hash))
        }
    }
}

/// Three-way merge of one value. `path` tracks the field location for conflicts.
///
/// Returns the merged value. Where a conflict was recorded, the returned
/// value is a placeholder and must not be used.
fn merge_node(
    path: &mut Vec<String>,
    base: Option<&UirNode>,
    ours: Option<&UirNode>,
    theirs: Option<&UirNode>,
    fields: &mut Vec<FieldConflict>,
) -> Option<UirNode> {
    if ours == theirs {
        return ours.cloned();
    }
    if base == ours {
        return theirs.cloned();
    }
    if base == theirs {
        return ours.cloned();
    }
    if let (Some(UirNode::Map(our_map)), Some(UirNode::Map(their_map))) = (ours, theirs) {
        let empty = BTreeMap::new();
        let base_map = match base {
            Some(UirNode::Map(map)) => map,
            _ => &empty,
        };
        let keys: BTreeSet<&String> = base_map
            .keys()
            .chain(our_map.keys())
            .chain(their_map.keys())
            .collect();
        let mut merged = BTreeMap::new();
        for key in keys {
            path.push(key.clone());
            let child = merge_node(
                path,
                base_map.get(key),
                our_map.get(key),
                their_map.get(key),
                fields,
            );
            path.pop();
            if let Some(value) = child {
                merged.insert(key.clone(), value);
            }
        }
        return Some(UirNode::Map(merged));
    }
    fields.push(FieldConflict {
        path: path.clone(),
        base: base.cloned(),
        ours: ours.cloned(),
        theirs: theirs.cloned(),
    });
    ours.cloned()
}

fn record_conflict<S: CasStore>(
    store: &mut S,
    entity: EntityId,
    base: Option<ContentHash>,
    ours: Option<ContentHash>,
    theirs: Option<ContentHash>,
    fields: Vec<FieldConflict>,
    conflicts: &mut Vec<Conflict>,
) -> Result<ContentHash, RepoError> {
    let conflict = Conflict {
        entity,
        base,
        ours,
        theirs,
        fields,
    };
    let hash = store_object(store, &Object::Conflict(conflict.clone()))?;
    conflicts.push(conflict);
    Ok(hash)
}

fn load_optional<S: CasStore>(
    store: &S,
    hash: Option<ContentHash>,
) -> Result<Option<UirNode>, RepoError> {
    match hash {
        Some(hash) => load_entity(store, &hash).map(Some),
        None => Ok(None),
    }
}
