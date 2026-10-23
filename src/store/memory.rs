//! In-memory store for tests and ephemeral repositories.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::{CasStore, RefStore, verify};
use crate::error::StoreError;
use crate::object::ContentHash;

/// A store that keeps everything in ordered maps.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    pub(crate) objects: BTreeMap<ContentHash, Vec<u8>>,
    refs: BTreeMap<String, ContentHash>,
}

impl CasStore for MemoryStore {
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError> {
        verify(&hash, data)?;
        self.objects.entry(hash).or_insert_with(|| data.to_vec());
        Ok(())
    }

    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        let data = self.objects.get(hash).ok_or(StoreError::NotFound(*hash))?;
        verify(hash, data)?;
        Ok(data.clone())
    }

    fn contains(&self, hash: &ContentHash) -> bool {
        self.objects.contains_key(hash)
    }
}

impl RefStore for MemoryStore {
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError> {
        Ok(self.refs.get(name).copied())
    }

    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError> {
        self.refs.insert(name.into(), hash);
        Ok(())
    }

    fn delete_ref(&mut self, name: &str) -> Result<bool, StoreError> {
        Ok(self.refs.remove(name).is_some())
    }

    fn list_refs(&self) -> Result<Vec<String>, StoreError> {
        Ok(self.refs.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_retrieves_by_hash() {
        let mut store = MemoryStore::default();
        let data = b"hello";
        let hash = ContentHash::of(data);
        store.put(hash, data).unwrap();
        assert!(store.contains(&hash));
        assert_eq!(store.get(&hash).unwrap(), data);
    }

    #[test]
    fn rejects_bytes_that_do_not_match_the_key() {
        let mut store = MemoryStore::default();
        let wrong = ContentHash::of(b"other");
        let err = store.put(wrong, b"hello").unwrap_err();
        assert!(matches!(err, StoreError::HashMismatch { .. }));
        assert!(!store.contains(&wrong));
    }

    #[test]
    fn deduplicates_identical_content() {
        let mut store = MemoryStore::default();
        let hash = ContentHash::of(b"same");
        store.put(hash, b"same").unwrap();
        store.put(hash, b"same").unwrap();
        assert_eq!(store.objects.len(), 1);
    }

    #[test]
    fn detects_corrupted_objects_on_read() {
        let mut store = MemoryStore::default();
        let hash = ContentHash::of(b"original");
        store.put(hash, b"original").unwrap();
        store.objects.insert(hash, b"tampered".to_vec());
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::HashMismatch { .. })
        ));
    }

    #[test]
    fn missing_object_is_not_found() {
        let store = MemoryStore::default();
        let hash = ContentHash::of(b"absent");
        assert_eq!(store.get(&hash), Err(StoreError::NotFound(hash)));
    }

    #[test]
    fn refs_round_trip_and_list_sorted() {
        let mut store = MemoryStore::default();
        let hash = ContentHash::of(b"commit");
        store.write_ref("main", hash).unwrap();
        store.write_ref("feature", hash).unwrap();
        assert_eq!(store.read_ref("main").unwrap(), Some(hash));
        assert_eq!(store.list_refs().unwrap(), ["feature", "main"]);
        assert!(store.delete_ref("main").unwrap());
        assert!(!store.delete_ref("main").unwrap());
    }
}
