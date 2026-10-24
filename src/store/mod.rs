//! Storage backends and the traits they implement.
//!
//! Objects are addressed by hash and every backend verifies bytes against that
//! hash on both write and read, so corruption is detected rather than returned.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{RepoError, StoreError};
use crate::object::{ContentHash, EntityId, Object, Tree, TreeRoot, wrong_kind};
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
pub use s3::{ConditionalClient, ObjectClient, S3Store};

/// A content-addressable object store.
pub trait CasStore {
    /// Stores `data` under `hash`. Fails if the bytes do not hash to `hash`.
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError>;

    /// Returns the bytes stored under `hash`, verified against it.
    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;

    /// Whether an object is stored under `hash`. A backend failure is an error,
    /// not `false`, so an unreachable store is never mistaken for a missing object.
    fn contains(&self, hash: &ContentHash) -> Result<bool, StoreError>;
}

/// A store that can list and delete its objects. Garbage collection needs this.
pub trait PruneStore: CasStore {
    /// Every object hash in the store, in no particular order.
    fn hashes(&self) -> Result<Vec<ContentHash>, StoreError>;

    /// Removes an object. Returns whether it was there.
    fn delete(&mut self, hash: &ContentHash) -> Result<bool, StoreError>;
}

/// Named branch refs, each pointing at a commit.
pub trait RefStore {
    /// The hash a ref points at, or `None` if the ref does not exist.
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError>;
    /// Points a ref at `hash`, creating it if needed. This is unconditional:
    /// use [`RefStore::compare_and_swap_ref`] when another writer might move
    /// the ref too.
    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError>;

    /// Points a ref at `new` only if it currently points at `expected`
    /// (`None` meaning the ref does not exist). Returns `false`, and changes
    /// nothing, when the ref has moved.
    fn compare_and_swap_ref(
        &mut self,
        name: &str,
        expected: Option<ContentHash>,
        new: ContentHash,
    ) -> Result<bool, StoreError>;

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

/// Entities per snapshot before it is split into shards.
pub(crate) const SHARD_THRESHOLD: usize = 256;

/// Loads a snapshot as one flat tree. A sharded root is read shard by shard.
pub(crate) fn load_tree<S: CasStore>(store: &S, hash: &ContentHash) -> Result<Tree, RepoError> {
    match load_object(store, hash)? {
        Object::Tree(tree) => Ok(tree),
        Object::TreeRoot(root) => {
            let mut entries = BTreeMap::new();
            for shard in root.shards.values() {
                match load_object(store, shard)? {
                    Object::Tree(tree) => entries.extend(tree.entries),
                    other => return Err(wrong_kind("tree shard", &other)),
                }
            }
            Ok(Tree { entries })
        }
        other => Err(wrong_kind("tree", &other)),
    }
}

/// The shard an entity belongs to: the first byte of the hash of its id. Hashing
/// spreads the shards evenly however the ids were assigned, so sequential ids
/// shard as well as random ones.
pub(crate) fn shard_of(id: &EntityId) -> u8 {
    ContentHash::of(id.as_bytes()).as_bytes()[0]
}

/// Stores a snapshot and returns its hash. Small snapshots are one tree. Larger
/// ones are split into shards by [`shard_of`], so a shard
/// that did not change keeps its hash and is not stored again.
pub(crate) fn store_tree<S: CasStore>(
    store: &mut S,
    entries: BTreeMap<EntityId, ContentHash>,
) -> Result<ContentHash, RepoError> {
    if entries.len() <= SHARD_THRESHOLD {
        return store_object(store, &Object::Tree(Tree { entries }));
    }
    let mut groups: BTreeMap<u8, BTreeMap<EntityId, ContentHash>> = BTreeMap::new();
    for (id, hash) in entries {
        groups.entry(shard_of(&id)).or_default().insert(id, hash);
    }
    let mut shards = BTreeMap::new();
    for (prefix, group) in groups {
        let shard = store_object(store, &Object::Tree(Tree { entries: group }))?;
        shards.insert(prefix, shard);
    }
    store_object(store, &Object::TreeRoot(TreeRoot { shards }))
}
