//! Commit-graph queries: loading commits, ancestry, descendants and merge bases.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::error::RepoError;
use crate::object::{Commit, ContentHash};
use crate::sign;
use crate::store::{CasStore, load_object};

/// Loads a commit, verifying its signature when it has one.
pub fn load_commit<S: CasStore>(store: &S, hash: &ContentHash) -> Result<Commit, RepoError> {
    let commit = match load_object(store, hash)? {
        crate::object::Object::Commit(commit) => commit,
        other => return Err(crate::object::wrong_kind("commit", &other)),
    };
    if commit
        .signature
        .as_ref()
        .is_some_and(|signature| !sign::verify(signature, &commit.signing_bytes()))
    {
        return Err(RepoError::InvalidSignature(*hash));
    }
    Ok(commit)
}

/// The tree hash a commit points at.
pub(crate) fn commit_tree<S: CasStore>(
    store: &S,
    hash: &ContentHash,
) -> Result<ContentHash, RepoError> {
    Ok(load_commit(store, hash)?.tree_hash)
}

/// Every commit reachable from `starts`, breadth first, each listed once.
pub fn ancestors<S: CasStore>(
    store: &S,
    starts: &[ContentHash],
) -> Result<Vec<ContentHash>, RepoError> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    for start in starts {
        if seen.insert(*start) {
            queue.push_back(*start);
        }
    }
    let mut order = Vec::new();
    while let Some(hash) = queue.pop_front() {
        for parent in load_commit(store, &hash)?.parents {
            if seen.insert(parent) {
                queue.push_back(parent);
            }
        }
        order.push(hash);
    }
    Ok(order)
}

/// Whether `ancestor` is reachable from `descendant` through one or more parent links.
pub fn is_ancestor<S: CasStore>(
    store: &S,
    ancestor: &ContentHash,
    descendant: &ContentHash,
) -> Result<bool, RepoError> {
    Ok(ancestors(store, core::slice::from_ref(descendant))?
        .iter()
        .skip(1)
        .any(|hash| hash == ancestor))
}

/// Commits that have `root` as an ancestor, among those reachable from `heads`.
pub fn descendants<S: CasStore>(
    store: &S,
    root: &ContentHash,
    heads: &[ContentHash],
) -> Result<Vec<ContentHash>, RepoError> {
    let mut children: BTreeMap<ContentHash, Vec<ContentHash>> = BTreeMap::new();
    for hash in ancestors(store, heads)? {
        for parent in load_commit(store, &hash)?.parents {
            children.entry(parent).or_default().push(hash);
        }
    }
    let mut seen = BTreeSet::from([*root]);
    let mut queue = VecDeque::from([*root]);
    let mut found = Vec::new();
    while let Some(hash) = queue.pop_front() {
        for child in children.get(&hash).map_or(&[][..], Vec::as_slice) {
            if seen.insert(*child) {
                found.push(*child);
                queue.push_back(*child);
            }
        }
    }
    Ok(found)
}

/// The best common ancestor of `a` and `b`, or `None` if their histories are unrelated.
///
/// With several best bases (a criss-cross history), the newest by timestamp
/// wins, ties broken by hash. Every peer therefore picks the same base.
pub fn find_merge_base<S: CasStore>(
    store: &S,
    a: &ContentHash,
    b: &ContentHash,
) -> Result<Option<ContentHash>, RepoError> {
    let from_a: BTreeSet<ContentHash> = ancestors(store, core::slice::from_ref(a))?
        .into_iter()
        .collect();
    let from_b: BTreeSet<ContentHash> = ancestors(store, core::slice::from_ref(b))?
        .into_iter()
        .collect();
    let common: Vec<ContentHash> = from_a.intersection(&from_b).copied().collect();

    // A common ancestor that another common ancestor descends from is superseded.
    let mut superseded = BTreeSet::new();
    for hash in &common {
        superseded.extend(
            ancestors(store, core::slice::from_ref(hash))?
                .into_iter()
                .skip(1),
        );
    }

    let mut best: Option<(u64, ContentHash)> = None;
    for hash in common.into_iter().filter(|hash| !superseded.contains(hash)) {
        let timestamp = load_commit(store, &hash)?.timestamp;
        if best.is_none_or(|current| (timestamp, hash) > current) {
            best = Some((timestamp, hash));
        }
    }
    Ok(best.map(|(_, hash)| hash))
}
