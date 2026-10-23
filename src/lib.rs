//! tpt-version: a content-addressable, structure-aware version control engine
//! for tpt-model entities.
//!
//! Entities are stored as canonical [`uir::UirNode`] trees, grouped into
//! snapshots ([`object::Tree`]) and linked into a Merkle DAG of
//! [`object::Commit`]s. Diffs and merges work on entity fields, not text lines.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod codec;
pub mod dag;
pub mod diff;
pub mod entity;
pub mod error;
pub mod merge;
pub mod object;
pub mod repo;
pub mod sign;
pub mod store;
pub mod uir;

pub use diff::{DiffEntry, EntityDiff, FieldChange};
pub use entity::Entity;
pub use error::{MergeError, ObjectError, RepoError, StoreError};
pub use merge::{MergeResult, Resolution};
pub use object::{ActorId, Commit, Conflict, ContentHash, EntityId, FieldConflict, Object, Tree};
pub use repo::{Clock, LogicalClock, Repository};
pub use sign::{Ed25519Signer, Signature, Signer};
pub use store::{CasStore, MemoryStore, RefStore};
pub use uir::{Scalar, UirNode};
