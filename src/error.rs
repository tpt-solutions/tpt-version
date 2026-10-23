//! Error types for the store, object decoding and repository layers.
//!
//! Merge failures are reported as [`RepoError`] variants, so [`MergeError`]
//! is an alias for it.

use alloc::string::String;
use core::fmt;

use crate::object::{ContentHash, EntityId};

/// A content-addressable or ref store failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// No object is stored under this hash.
    NotFound(ContentHash),
    /// Bytes do not hash to the key they were stored or fetched under.
    HashMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
    /// The storage backend failed: I/O, network, or a malformed ref file.
    Backend(String),
}

/// Stored bytes could not be decoded as the object they claim to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectError {
    /// The input ended before a field was complete.
    Truncated,
    /// The leading type tag is not a known object kind.
    UnknownTag(u8),
    /// The object decoded, but it is not the kind the caller asked for.
    WrongKind {
        expected: &'static str,
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
pub enum RepoError {
    Store(StoreError),
    Object(ObjectError),
    InvalidBranchName(String),
    BranchExists(String),
    BranchNotFound(String),
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
            Self::InvalidBranchName(name) => write!(f, "invalid branch name {name:?}"),
            Self::BranchExists(name) => write!(f, "branch {name:?} already exists"),
            Self::BranchNotFound(name) => write!(f, "branch {name:?} not found"),
            Self::CannotDeleteCurrentBranch(name) => {
                write!(f, "cannot delete the checked-out branch {name:?}")
            }
            Self::DirtyWorkingCopy => write!(f, "working copy has uncommitted changes"),
            Self::UnresolvedConflicts(count) => {
                write!(
                    f,
                    "{count} unresolved conflict(s) must be resolved before committing"
                )
            }
            Self::ConflictPending(id) => write!(f, "entity {id} has an unresolved conflict"),
            Self::NoConflict(id) => write!(f, "entity {id} has no pending conflict"),
            Self::InvalidSignature(hash) => write!(f, "commit {hash} has an invalid signature"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StoreError {}

#[cfg(feature = "std")]
impl std::error::Error for ObjectError {}

#[cfg(feature = "std")]
impl std::error::Error for RepoError {}
