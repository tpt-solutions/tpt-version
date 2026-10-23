//! Maintenance: checking the store's integrity ([`fsck`]) and finding the
//! objects that nothing can reach (used by `Repository::gc`).

use alloc::collections::{BTreeSet, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{RepoError, StoreError};
use crate::object::{ContentHash, Object};
use crate::sign;
use crate::store::{CasStore, RefStore, load_object};

/// The hashes an object refers to, which the object needs to be complete.
pub(crate) fn children(object: &Object) -> Vec<ContentHash> {
    match object {
        Object::Blob(_) | Object::Entity(_) => Vec::new(),
        Object::Tree(tree) => tree.entries.values().copied().collect(),
        Object::Commit(commit) => {
            let mut hashes = alloc::vec![commit.tree_hash];
            hashes.extend(&commit.parents);
            hashes
        }
        Object::Conflict(conflict) => [conflict.base, conflict.ours, conflict.theirs]
            .into_iter()
            .flatten()
            .collect(),
    }
}

/// Objects reachable from `roots`, following every reference an object holds.
///
/// A missing object is an error, not a silent gap, so a broken graph stops the
/// caller before it can act on a partial answer.
pub(crate) fn reachable<S: CasStore>(
    store: &S,
    roots: impl IntoIterator<Item = ContentHash>,
) -> Result<BTreeSet<ContentHash>, RepoError> {
    let mut seen = BTreeSet::new();
    let mut queue: VecDeque<ContentHash> = roots.into_iter().collect();
    while let Some(hash) = queue.pop_front() {
        if !seen.insert(hash) {
            continue;
        }
        let object = load_object(store, &hash)?;
        queue.extend(children(&object));
    }
    Ok(seen)
}

/// A problem found by [`fsck`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// A branch ref could not be read, for example because its file is corrupt.
    BrokenRef {
        /// The ref's name.
        name: String,
        /// Why it could not be read.
        reason: String,
    },
    /// An object is referenced but is not in the store.
    MissingObject {
        /// The object that is missing.
        hash: ContentHash,
        /// The object that refers to it. `None` for a ref's target.
        referenced_by: Option<ContentHash>,
    },
    /// An object is stored, but its bytes do not hash to its name or do not
    /// decode as the object they claim to be.
    Corrupt {
        /// The object's name.
        hash: ContentHash,
        /// Why it was rejected.
        reason: String,
    },
    /// A commit's signature does not verify against its contents.
    BadSignature(ContentHash),
}

/// The outcome of [`fsck`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FsckReport {
    /// How many distinct reachable objects were read and decoded.
    pub objects_checked: usize,
    /// Everything that is wrong. Empty when the store is clean.
    pub problems: Vec<Problem>,
}

impl FsckReport {
    /// Whether no problems were found.
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Checks every object reachable from a branch: that it is present, hashes to
/// its name, decodes, and (for commits) carries a valid signature.
///
/// Problems are collected rather than returned on the first one, so one run
/// shows the full damage. Only a failure to list the refs is an error.
pub fn fsck<S: CasStore + RefStore>(store: &S) -> Result<FsckReport, RepoError> {
    let mut report = FsckReport::default();
    let mut seen = BTreeSet::new();
    let mut queue: VecDeque<(ContentHash, Option<ContentHash>)> = VecDeque::new();

    for name in store.list_refs()? {
        match store.read_ref(&name) {
            Ok(Some(hash)) => queue.push_back((hash, None)),
            // Removed between listing and reading, so there is nothing to check.
            Ok(None) => {}
            Err(error) => report.problems.push(Problem::BrokenRef {
                name,
                reason: error.to_string(),
            }),
        }
    }

    while let Some((hash, referenced_by)) = queue.pop_front() {
        if !seen.insert(hash) {
            continue;
        }
        let data = match store.get(&hash) {
            Ok(data) => data,
            Err(StoreError::NotFound(_)) => {
                report.problems.push(Problem::MissingObject {
                    hash,
                    referenced_by,
                });
                continue;
            }
            Err(error) => {
                report.problems.push(Problem::Corrupt {
                    hash,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let object = match Object::decode(&data) {
            Ok(object) => object,
            Err(error) => {
                report.problems.push(Problem::Corrupt {
                    hash,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        report.objects_checked += 1;
        if let Object::Commit(commit) = &object {
            let bad_signature = commit
                .signature
                .as_ref()
                .is_some_and(|signature| !sign::verify(signature, &commit.signing_bytes()));
            if bad_signature {
                report.problems.push(Problem::BadSignature(hash));
            }
        }
        for child in children(&object) {
            queue.push_back((child, Some(hash)));
        }
    }
    Ok(report)
}

/// The outcome of `Repository::gc`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Objects kept because something can reach them.
    pub kept: usize,
    /// Unreachable objects removed from the store.
    pub removed: usize,
}
