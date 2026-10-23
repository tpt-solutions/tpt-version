//! S3-backed object and ref store for distributed collaboration.
//!
//! The network client is injected through [`ObjectClient`] and, for branch
//! refs, [`ConditionalClient`], so this crate carries no SDK dependency.
//! Callers implement the traits over whichever S3 client they use.

use alloc::string::String;
use alloc::vec::Vec;

use super::{CasStore, RefStore, verify};
use crate::error::StoreError;
use crate::object::ContentHash;

/// The S3 operations the store needs. Errors are plain strings so any SDK can implement this.
pub trait ObjectClient {
    /// Uploads `body` under `key`, replacing any existing object.
    fn put_object(&mut self, key: &str, body: &[u8]) -> Result<(), String>;

    /// Returns `Ok(None)` when the key does not exist.
    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>, String>;

    /// Whether an object exists under `key`, without fetching its body.
    fn head_object(&self, key: &str) -> Result<bool, String>;
}

/// Conditional writes, needed for branch refs.
///
/// S3 expresses these with ETags: `If-None-Match: *` writes only when the key is
/// absent, and `If-Match` writes only while the object still has the ETag that
/// was read. Implementations map those headers to the methods below.
pub trait ConditionalClient: ObjectClient {
    /// Returns the body and its ETag, or `Ok(None)` when the key does not exist.
    fn get_object_with_etag(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, String>;

    /// Uploads `body` only if the current object matches `expected`: its ETag,
    /// or no object at all when `expected` is `None`. Returns `Ok(false)` when
    /// the precondition fails (HTTP 412).
    fn put_object_if(
        &mut self,
        key: &str,
        body: &[u8],
        expected: Option<&str>,
    ) -> Result<bool, String>;

    /// Deletes `key`. Returns whether an object was there to delete.
    fn delete_object(&mut self, key: &str) -> Result<bool, String>;

    /// Every key that starts with `prefix`, in any order.
    fn list_keys(&self, prefix: &str) -> Result<Vec<String>, String>;
}

/// A store that keeps objects under `<prefix>objects/<hash>` and branch refs
/// under `<prefix>refs/heads/<name>`.
pub struct S3Store<C> {
    client: C,
    prefix: String,
}

impl<C> S3Store<C> {
    /// Wraps `client`. Keys go under `prefix`, which gets a trailing `/` if non-empty.
    pub fn new(client: C, prefix: impl Into<String>) -> Self {
        let mut prefix = prefix.into();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        Self { client, prefix }
    }

    fn object_key(&self, hash: &ContentHash) -> String {
        format!("{}objects/{hash}", self.prefix)
    }

    fn refs_prefix(&self) -> String {
        format!("{}refs/heads/", self.prefix)
    }

    fn ref_key(&self, name: &str) -> Result<String, StoreError> {
        let unsafe_name = name.is_empty()
            || name.starts_with('/')
            || name.split('/').any(|part| part.is_empty() || part == "..");
        if unsafe_name {
            return Err(StoreError::Backend(format!("unsafe ref name {name:?}")));
        }
        Ok(format!("{}{name}", self.refs_prefix()))
    }
}

fn backend(message: String) -> StoreError {
    StoreError::Backend(message)
}

impl<C: ObjectClient> CasStore for S3Store<C> {
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError> {
        verify(&hash, data)?;
        let key = self.object_key(&hash);
        if self.client.head_object(&key).map_err(backend)? {
            return Ok(());
        }
        self.client.put_object(&key, data).map_err(backend)
    }

    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        let data = self
            .client
            .get_object(&self.object_key(hash))
            .map_err(backend)?
            .ok_or(StoreError::NotFound(*hash))?;
        verify(hash, &data)?;
        Ok(data)
    }

    fn contains(&self, hash: &ContentHash) -> Result<bool, StoreError> {
        self.client
            .head_object(&self.object_key(hash))
            .map_err(backend)
    }
}

impl<C: ConditionalClient> RefStore for S3Store<C> {
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError> {
        let key = self.ref_key(name)?;
        match self.client.get_object_with_etag(&key).map_err(backend)? {
            Some((body, _etag)) => parse_ref(name, &body).map(Some),
            None => Ok(None),
        }
    }

    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError> {
        let key = self.ref_key(name)?;
        self.client
            .put_object(&key, format!("{hash}\n").as_bytes())
            .map_err(backend)
    }

    fn compare_and_swap_ref(
        &mut self,
        name: &str,
        expected: Option<ContentHash>,
        new: ContentHash,
    ) -> Result<bool, StoreError> {
        let key = self.ref_key(name)?;
        let (current, etag) = match self.client.get_object_with_etag(&key).map_err(backend)? {
            Some((body, etag)) => (Some(parse_ref(name, &body)?), Some(etag)),
            None => (None, None),
        };
        if current != expected {
            return Ok(false);
        }
        // The write succeeds only if the object still has the ETag that was just
        // read, so a concurrent writer makes this return `false` instead of
        // being overwritten.
        self.client
            .put_object_if(&key, format!("{new}\n").as_bytes(), etag.as_deref())
            .map_err(backend)
    }

    fn delete_ref(&mut self, name: &str) -> Result<bool, StoreError> {
        let key = self.ref_key(name)?;
        self.client.delete_object(&key).map_err(backend)
    }

    fn list_refs(&self) -> Result<Vec<String>, StoreError> {
        let prefix = self.refs_prefix();
        let mut names: Vec<String> = self
            .client
            .list_keys(&prefix)
            .map_err(backend)?
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix).map(String::from))
            .collect();
        names.sort();
        Ok(names)
    }
}

fn parse_ref(name: &str, body: &[u8]) -> Result<ContentHash, StoreError> {
    core::str::from_utf8(body)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| StoreError::Backend(format!("corrupt ref {name:?}")))
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
        assert!(store.contains(&hash).unwrap());
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
