//! Per-entity history and field-level blame, following the first-parent chain
//! from a commit. Merge commits contribute their first parent's history, and
//! the second parent's changes show up at the merge commit itself.

use alloc::string::String;
use alloc::vec::Vec;

use crate::dag;
use crate::error::RepoError;
use crate::object::{ContentHash, EntityId};
use crate::store::{CasStore, load_entity, load_tree};
use crate::uir::UirNode;

/// A commit that changed an entity, and what the entity held afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityVersion {
    /// The commit that changed the entity.
    pub commit: ContentHash,
    /// The entity's content after the commit, or `None` if the commit removed it.
    pub content: Option<ContentHash>,
}

/// The commit that last set one field of an entity to its current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldBlame {
    /// The field's path from the entity root.
    pub path: Vec<String>,
    /// The commit that introduced the field's current value.
    pub commit: ContentHash,
}

/// The commits that changed `id`, newest first, starting from `head`.
pub(crate) fn entity_history<S: CasStore>(
    store: &S,
    head: ContentHash,
    id: &EntityId,
) -> Result<Vec<EntityVersion>, RepoError> {
    let mut versions = Vec::new();
    let mut current = Some(head);
    while let Some(hash) = current {
        let commit = dag::load_commit(store, &hash)?;
        let here = entity_in(store, &commit.tree_hash, id)?;
        let parent = commit.parents.first().copied();
        let before = match parent {
            Some(parent) => entity_in(store, &dag::commit_tree(store, &parent)?, id)?,
            None => None,
        };
        if here != before {
            versions.push(EntityVersion {
                commit: hash,
                content: here,
            });
        }
        current = parent;
    }
    Ok(versions)
}

/// For each field of the entity at `head`, the commit that set its current value.
///
/// A field's value is set by the oldest change in the run of changes, newest
/// first, that still hold the current value. A change that removes the entity
/// ends the run.
pub(crate) fn field_blame<S: CasStore>(
    store: &S,
    head: ContentHash,
    id: &EntityId,
) -> Result<Vec<FieldBlame>, RepoError> {
    let head_tree = dag::commit_tree(store, &head)?;
    let Some(current_hash) = entity_in(store, &head_tree, id)? else {
        return Ok(Vec::new());
    };
    let current = load_entity(store, &current_hash)?;

    let mut versions = Vec::new();
    for version in entity_history(store, head, id)? {
        let node = match version.content {
            Some(hash) => Some(load_entity(store, &hash)?),
            None => None,
        };
        versions.push((version.commit, node));
    }

    let mut paths = Vec::new();
    leaves(&current, &mut Vec::new(), &mut paths);
    let mut blame = Vec::new();
    for path in paths {
        let wanted = lookup(&current, &path);
        let mut commit = None;
        for (version_commit, node) in &versions {
            match node {
                Some(node) if lookup(node, &path) == wanted => commit = Some(*version_commit),
                _ => break,
            }
        }
        if let Some(commit) = commit {
            blame.push(FieldBlame { path, commit });
        }
    }
    Ok(blame)
}

fn entity_in<S: CasStore>(
    store: &S,
    tree: &ContentHash,
    id: &EntityId,
) -> Result<Option<ContentHash>, RepoError> {
    Ok(load_tree(store, tree)?.entries.get(id).copied())
}

/// Collects the path to every scalar or list leaf under `node`.
fn leaves(node: &UirNode, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match node {
        UirNode::Map(fields) => {
            for (name, value) in fields {
                path.push(name.clone());
                leaves(value, path, out);
                path.pop();
            }
        }
        UirNode::Scalar(_) | UirNode::List(_) => out.push(path.clone()),
    }
}

/// The value at `path` under `node`, if there is one.
fn lookup<'a>(node: &'a UirNode, path: &[String]) -> Option<&'a UirNode> {
    let mut current = node;
    for name in path {
        match current {
            UirNode::Map(fields) => current = fields.get(name)?,
            _ => return None,
        }
    }
    Some(current)
}

/// The newest commit reachable from `head` whose timestamp is at or before
/// `timestamp`. Ties on timestamp go to the larger hash, so the answer is the
/// same on every machine.
pub(crate) fn commit_as_of<S: CasStore>(
    store: &S,
    head: ContentHash,
    timestamp: u64,
) -> Result<Option<ContentHash>, RepoError> {
    let mut best: Option<(u64, ContentHash)> = None;
    for hash in dag::ancestors(store, &[head])? {
        let at = dag::load_commit(store, &hash)?.timestamp;
        if at <= timestamp && best.is_none_or(|current| (at, hash) > current) {
            best = Some((at, hash));
        }
    }
    Ok(best.map(|(_, hash)| hash))
}
