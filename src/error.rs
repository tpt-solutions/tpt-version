//! Error types for the store, object decoding and repository layers.
//!
//! Merge failures are reported as [`RepoError`] variants, so [`MergeError`]
//! is an alias for it.

use alloc::string::String;
use core::fmt;

use crate::object::{ContentHash, EntityId};

/// A content-addressable or ref store failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreError {
    /// No object is stored under this hash.
    NotFound(ContentHash),
    /// Bytes do not hash to the key they were stored or fetched under.
    HashMismatch {
        /// The hash the bytes were stored or fetched under.
        expected: ContentHash,
        /// The hash the bytes actually have.
        actual: ContentHash,
    },
    /// The storage backend failed: I/O, network, or a malformed ref file.
    Backend(String),
}

/// Stored bytes could not be decoded as the object they claim to be.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObjectError {
    /// The input ended before a field was complete.
    Truncated,
    /// The leading type tag is not a known object kind.
    UnknownTag(u8),
    /// The object decoded, but it is not the kind the caller asked for.
    WrongKind {
        /// The kind the caller asked for.
        expected: &'static str,
        /// The kind that was actually stored.
        found: &'static str,
    },
    /// A field holds an invalid value, such as a bad flag or hex digit.
    InvalidValue(&'static str),
    /// A text field is not valid UTF-8.
    InvalidUtf8,
    /// Bytes remain after the object ends.
    TrailingBytes,
}

/// A repository operation failed. Merge failures use these variants too.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RepoError {
    /// A store operation failed.
    Store(StoreError),
    /// Stored bytes could not be decoded.
    Object(ObjectError),
    /// The branch name breaks the naming rules.
    InvalidBranchName(String),
    /// A branch with this name already exists.
    BranchExists(String),
    /// The new branch name collides with an existing one: it differs only in
    /// case, or one is a path prefix of the other (`a` and `a/b`).
    BranchNameClash {
        /// The name that was asked for.
        name: String,
        /// The existing branch it clashes with.
        existing: String,
    },
    /// No branch with this name exists.
    BranchNotFound(String),
    /// The checked-out branch cannot be deleted.
    CannotDeleteCurrentBranch(String),
    /// The working copy has changes that are not in HEAD.
    DirtyWorkingCopy,
    /// A merge left conflicts that must be resolved before committing.
    UnresolvedConflicts(usize),
    /// The entity has an unresolved conflict and cannot be edited yet.
    ConflictPending(EntityId),
    /// The entity has no pending conflict to resolve.
    NoConflict(EntityId),
    /// A commit's signature does not verify against its contents.
    InvalidSignature(ContentHash),
    /// The branch moved after this repository last read it, so the update was
    /// refused. Reopen the branch and try again.
    RefMoved(String),
    /// The entity's stored form loads back under a different id than the one
    /// it was staged with.
    IdMismatch(EntityId),
    /// A `pre_commit` hook refused the commit, with its reason.
    HookRejected(String),
    /// A tag with this name already exists. Tags are immutable.
    TagExists(String),
    /// No tag with this name exists.
    TagNotFound(String),
    /// The destination's branch has commits the source does not, so updating it
    /// would lose them. Fetch and merge first, or force-update outside this API.
    NotFastForward(String),
    /// The operation needs a finished merge, and one is still staged.
    MergeInProgress,
    /// There is no head commit to amend.
    NothingToAmend,
    /// The branch is governed by a trust policy, and a commit that would land on
    /// it, or be merged into it, is not signed by a trusted key.
    Untrusted(String),
}

/// Merge failures are reported as [`RepoError`] variants.
pub type MergeError = RepoError;

impl From<StoreError> for RepoError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<ObjectError> for RepoError {
    fn from(error: ObjectError) -> Self {
        Self::Object(error)
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(hash) => write!(f, "no object with hash {hash}"),
            Self::HashMismatch { expected, actual } => {
                write!(f, "object hashes to {actual}, expected {expected}")
            }
            Self::Backend(message) => write!(f, "storage backend failed: {message}"),
        }
    }
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "object is truncated"),
            Self::UnknownTag(tag) => write!(f, "unknown object tag {tag}"),
            Self::WrongKind { expected, found } => {
                write!(f, "expected a {expected} object, found a {found}")
            }
            Self::InvalidValue(what) => write!(f, "invalid {what}"),
            Self::InvalidUtf8 => write!(f, "text field is not valid UTF-8"),
            Self::TrailingBytes => write!(f, "bytes remain after the object"),
        }
    }
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Object(error) => write!(f, "{error}"),
            Self::InvalidBranchName(name) => write!(
                f,
                "invalid branch name {name:?}: use letters, digits, dot, underscore, dash and slash, with no leading or trailing separator, and do not use the tags/ or reviews/ prefix"
            ),
            Self::BranchExists(name) => write!(f, "branch {name:?} already exists"),
            Self::BranchNameClash { name, existing } => {
                write!(
                    f,
                    "branch {name:?} clashes with existing branch {existing:?}"
                )
            }
            Self::BranchNotFound(name) => write!(
                f,
                "branch {name:?} not found; list_branches shows the branches that exist"
            ),
            Self::CannotDeleteCurrentBranch(name) => {
                write!(f, "cannot delete the checked-out branch {name:?}")
            }
            Self::DirtyWorkingCopy => write!(
                f,
                "working copy has uncommitted changes; commit them, or reset_hard to discard them"
            ),
            Self::UnresolvedConflicts(count) => {
                write!(
                    f,
                    "{count} unresolved conflict(s); settle each with resolve_conflict, then commit"
                )
            }
            Self::ConflictPending(id) => write!(
                f,
                "entity {id} has an unresolved conflict; settle it with resolve_conflict"
            ),
            Self::NoConflict(id) => write!(f, "entity {id} has no pending conflict"),
            Self::InvalidSignature(hash) => write!(f, "commit {hash} has an invalid signature"),
            Self::RefMoved(name) => write!(
                f,
                "branch {name:?} moved since this repository read it; reopen it and retry"
            ),
            Self::IdMismatch(id) => write!(f, "entity {id} does not load back under its own id"),
            Self::HookRejected(reason) => write!(f, "commit refused by a hook: {reason}"),
            Self::TagExists(name) => write!(
                f,
                "tag {name:?} already exists; tags are immutable, so choose another name"
            ),
            Self::TagNotFound(name) => write!(
                f,
                "tag {name:?} not found; list_tags shows the tags that exist"
            ),
            Self::NotFastForward(name) => {
                write!(
                    f,
                    "branch {name:?} has commits the other side lacks, so this is not a fast-forward; fetch and merge first"
                )
            }
            Self::MergeInProgress => {
                write!(
                    f,
                    "a merge is staged; commit it, or discard it with reset_hard"
                )
            }
            Self::NothingToAmend => write!(f, "there is no commit to amend"),
            Self::Untrusted(branch) => {
                write!(
                    f,
                    "branch {branch:?} accepts only commits signed by a trusted key; sign with a trusted key, or change the trust policy"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StoreError {}

#[cfg(feature = "std")]
impl std::error::Error for ObjectError {}

#[cfg(feature = "std")]
impl std::error::Error for RepoError {}
