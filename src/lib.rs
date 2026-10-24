//! The crate README, which also serves as the crate overview. Its Quick Start
//! is compiled as a doctest.
#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod codec;
pub mod dag;
pub mod diff;
pub mod entity;
pub mod error;
pub mod export;
pub mod history;
pub mod maintenance;
pub mod merge;
pub mod object;
pub mod oplog;
pub mod prelude;
pub mod repo;
pub mod sign;
pub mod store;
pub mod sync;
mod text_merge;
pub mod trust;
pub mod uir;
pub mod validate;

pub use diff::{DiffEntry, EntityDiff, FieldChange};
pub use entity::{Entity, UirValue};

pub use error::{MergeError, ObjectError, RepoError, StoreError};
pub use merge::{MergeResult, Resolution};
pub use object::{
    ActorId, Commit, Conflict, ContentHash, EntityId, FieldConflict, Object, Review, Tree, Verdict,
};
pub use repo::{Clock, LogicalClock, MergeReview, Repository};
pub use sign::{Ed25519Signer, Signature, Signer};
pub use store::{CasStore, MemoryStore, RefStore};
/// The `#[derive(Entity)]` macro. It shares a name with the trait, which lives
/// in a different namespace.
#[cfg(feature = "derive")]
pub use tpt_version_derive::Entity;
pub use uir::{Scalar, UirNode};
pub use validate::Validator;
