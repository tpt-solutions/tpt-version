//! Storage backends and the traits they implement.
//!
//! Objects are addressed by hash and every backend verifies bytes against that
//! hash on both write and read, so corruption is detected rather than returned.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{RepoError, StoreError};
use crate::object::{ContentHash, Object, Tree, wrong_kind};
use crate::uir::UirNode;

mod memory;
pub use memory::MemoryStore;

#[cfg(feature = "std")]
mod fs;
#[cfg(feature = "std")]
pub use fs::FileSystemStore;

#[cfg(feature = "s3")]
mod s3;
#[cfg(feature = "s3")]
pub use s3::{ObjectClient, S3Store};

/// A content-addressable object store.
pub trait CasStore {
    /// Stores `data` under `hash`. Fails if the bytes do not hash to `hash`.
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError>;

    /// Returns the bytes stored under `hash`, verified against it.
    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;

    /// Whether an object is stored under `hash`.
    fn contains(&self, hash: &ContentHash) -> bool;
}

/// Named branch refs, each pointing at a commit.
pub trait RefStore {
    /// The hash a ref points at, or `None` if the ref does not exist.
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError>;
    /// Points a ref at `hash`, creating it if needed.
    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError>;

    /// Returns whether a ref was removed.
    fn delete_ref(&mut self, name: &str) -> Result<bool, StoreError>;

    /// All ref names, sorted.
    fn list_refs(&self) -> Result<Vec<String>, StoreError>;
}

/// Checks that `data` hashes to `expected`.
pub(crate) fn verify(expected: &ContentHash, data: &[u8]) -> Result<(), StoreError> {
    let actual = ContentHash::of(data);
    if actual == *expected {
        Ok(())
    } else {
        Err(StoreError::HashMismatch {
            expected: *expected,
            actual,
        })
    }
}

/// Encodes and stores an object, returning its hash.
pub(crate) fn store_object<S: CasStore>(
    store: &mut S,
    object: &Object,
) -> Result<ContentHash, RepoError> {
    let data = object.encode();
    let hash = ContentHash::of(&data);
    store.put(hash, &data)?;
    Ok(hash)
}

/// Fetches and decodes an object. The store verifies the hash.
pub(crate) fn load_object<S: CasStore>(store: &S, hash: &ContentHash) -> Result<Object, RepoError> {
    Ok(Object::decode(&store.get(hash)?)?)
}

pub(crate) fn load_entity<S: CasStore>(
    store: &S,
    hash: &ContentHash,
) -> Result<UirNode, RepoError> {
    match load_object(store, hash)? {
        Object::Entity(node) => Ok(node),
        other => Err(wrong_kind("entity", &other)),
    }
}

pub(crate) fn load_tree<S: CasStore>(store: &S, hash: &ContentHash) -> Result<Tree, RepoError> {
    match load_object(store, hash)? {
        Object::Tree(tree) => Ok(tree),
        other => Err(wrong_kind("tree", &other)),
    }
}
