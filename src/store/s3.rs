//! S3-backed object store for distributed collaboration.
//!
//! The network client is injected through [`ObjectClient`], so this crate
//! carries no SDK dependency. Callers implement the trait over whichever S3
//! client they use.

use alloc::string::String;
use alloc::vec::Vec;

use super::{CasStore, verify};
use crate::error::StoreError;
use crate::object::ContentHash;

/// The S3 operations the store needs. Errors are plain strings so any SDK can implement this.
pub trait ObjectClient {
    fn put_object(&mut self, key: &str, body: &[u8]) -> Result<(), String>;

    /// Returns `Ok(None)` when the key does not exist.
    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>, String>;

    fn head_object(&self, key: &str) -> Result<bool, String>;
}

/// An object store that keeps objects under `<prefix>objects/<hash>`.
pub struct S3Store<C> {
    client: C,
    prefix: String,
}

impl<C: ObjectClient> S3Store<C> {
    /// Wraps `client`. Objects go under `prefix`, which gets a trailing `/` if non-empty.
    pub fn new(client: C, prefix: impl Into<String>) -> Self {
        let mut prefix = prefix.into();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        Self { client, prefix }
    }

    fn key(&self, hash: &ContentHash) -> String {
        format!("{}objects/{hash}", self.prefix)
    }
}

fn backend(message: String) -> StoreError {
    StoreError::Backend(message)
}

impl<C: ObjectClient> CasStore for S3Store<C> {
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError> {
        verify(&hash, data)?;
        let key = self.key(&hash);
        if self.client.head_object(&key).map_err(backend)? {
            return Ok(());
        }
        self.client.put_object(&key, data).map_err(backend)
    }

    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        let data = self
            .client
            .get_object(&self.key(hash))
            .map_err(backend)?
            .ok_or(StoreError::NotFound(*hash))?;
        verify(hash, &data)?;
        Ok(data)
    }

    fn contains(&self, hash: &ContentHash) -> bool {
        self.client.head_object(&self.key(hash)).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    /// An in-memory stand-in for an S3 bucket.
    #[derive(Default)]
    struct FakeBucket {
        objects: BTreeMap<String, Vec<u8>>,
    }

    impl ObjectClient for FakeBucket {
        fn put_object(&mut self, key: &str, body: &[u8]) -> Result<(), String> {
            self.objects.insert(key.into(), body.to_vec());
            Ok(())
        }

        fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.objects.get(key).cloned())
        }

        fn head_object(&self, key: &str) -> Result<bool, String> {
            Ok(self.objects.contains_key(key))
        }
    }

    #[test]
    fn round_trips_under_prefix() {
        let mut store = S3Store::new(FakeBucket::default(), "repo-a");
        let hash = ContentHash::of(b"blob");
        store.put(hash, b"blob").unwrap();
        assert!(store.contains(&hash));
        assert_eq!(store.get(&hash).unwrap(), b"blob");
        assert!(
            store
                .client
                .objects
                .contains_key(&format!("repo-a/objects/{hash}"))
        );
    }

    #[test]
    fn rejects_mismatched_bytes_before_upload() {
        let mut store = S3Store::new(FakeBucket::default(), "");
        let hash = ContentHash::of(b"expected");
        assert!(store.put(hash, b"actual").is_err());
        assert!(store.client.objects.is_empty());
    }
}
