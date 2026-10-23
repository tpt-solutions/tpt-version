//! End to end over `S3Store`: two repositories share one bucket, and the
//! branch ref behaves like a compare-and-swap across them.
#![cfg(feature = "s3")]

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{author, vertex};
use tpt_version::store::{CasStore, ConditionalClient, ObjectClient, RefStore, S3Store};
use tpt_version::{ContentHash, LogicalClock, RepoError, Repository, StoreError};

#[derive(Default)]
struct Bucket {
    /// Body and a version number, which doubles as the ETag.
    objects: BTreeMap<String, (Vec<u8>, u64)>,
    next_version: u64,
    failing: bool,
}

/// A cloneable handle, so several stores can share one bucket.
#[derive(Clone, Default)]
struct SharedBucket(Arc<Mutex<Bucket>>);

impl SharedBucket {
    fn fail(&self, failing: bool) {
        self.0.lock().unwrap().failing = failing;
    }

    fn check(bucket: &Bucket) -> Result<(), String> {
        if bucket.failing {
            Err("network unreachable".into())
        } else {
            Ok(())
        }
    }
}

impl ObjectClient for SharedBucket {
    fn put_object(&mut self, key: &str, body: &[u8]) -> Result<(), String> {
        let mut bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        bucket.next_version += 1;
        let version = bucket.next_version;
        bucket.objects.insert(key.into(), (body.to_vec(), version));
        Ok(())
    }

    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        Ok(bucket.objects.get(key).map(|(body, _)| body.clone()))
    }

    fn head_object(&self, key: &str) -> Result<bool, String> {
        let bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        Ok(bucket.objects.contains_key(key))
    }
}

impl ConditionalClient for SharedBucket {
    fn get_object_with_etag(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, String> {
        let bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        Ok(bucket
            .objects
            .get(key)
            .map(|(body, version)| (body.clone(), version.to_string())))
    }

    fn put_object_if(
        &mut self,
        key: &str,
        body: &[u8],
        expected: Option<&str>,
    ) -> Result<bool, String> {
        let mut bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        let current = bucket
            .objects
            .get(key)
            .map(|(_, version)| version.to_string());
        if current.as_deref() != expected {
            return Ok(false);
        }
        bucket.next_version += 1;
        let version = bucket.next_version;
        bucket.objects.insert(key.into(), (body.to_vec(), version));
        Ok(true)
    }

    fn delete_object(&mut self, key: &str) -> Result<bool, String> {
        let mut bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        Ok(bucket.objects.remove(key).is_some())
    }

    fn list_keys(&self, prefix: &str) -> Result<Vec<String>, String> {
        let bucket = self.0.lock().unwrap();
        SharedBucket::check(&bucket)?;
        Ok(bucket
            .objects
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect())
    }
}

fn open(bucket: &SharedBucket, prefix: &str) -> Repository<S3Store<SharedBucket>, LogicalClock> {
    Repository::open(
        S3Store::new(bucket.clone(), prefix),
        LogicalClock::starting_at(1),
        "main",
    )
    .unwrap()
}

#[test]
fn stale_writer_on_a_shared_bucket_is_refused() {
    let bucket = SharedBucket::default();
    let mut a = open(&bucket, "repo");
    // Opened while `main` is unborn, so it expects no ref.
    let mut b = open(&bucket, "repo");

    a.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let winner = a.commit("from a", &author()).unwrap();

    b.put_entity(&vertex(2, [1.0; 3])).unwrap();
    assert_eq!(
        b.commit("from b", &author()),
        Err(RepoError::RefMoved("main".into()))
    );

    let mut reader = open(&bucket, "repo");
    reader.switch_branch("main").unwrap();
    assert_eq!(reader.head(), Some(winner));
    assert!(
        reader
            .entity::<common::Vertex>(&common::id(1))
            .unwrap()
            .is_some()
    );
}

#[test]
fn compare_and_swap_respects_the_expected_value() {
    let bucket = SharedBucket::default();
    let mut store = S3Store::new(bucket.clone(), "");
    let first = ContentHash::of(b"first");
    let second = ContentHash::of(b"second");

    assert!(store.compare_and_swap_ref("main", None, first).unwrap());
    assert!(!store.compare_and_swap_ref("main", None, second).unwrap());
    assert!(
        store
            .compare_and_swap_ref("main", Some(first), second)
            .unwrap()
    );
    assert_eq!(store.read_ref("main").unwrap(), Some(second));
}

#[test]
fn prefixes_keep_repositories_apart() {
    let bucket = SharedBucket::default();
    let mut one = S3Store::new(bucket.clone(), "one");
    let mut two = S3Store::new(bucket.clone(), "two");
    let hash = ContentHash::of(b"x");
    one.write_ref("main", hash).unwrap();
    two.write_ref("dev", hash).unwrap();

    assert_eq!(one.list_refs().unwrap(), ["main"]);
    assert_eq!(two.list_refs().unwrap(), ["dev"]);
    assert!(one.delete_ref("main").unwrap());
    assert!(!one.delete_ref("main").unwrap());
    assert_eq!(two.list_refs().unwrap(), ["dev"]);
}

#[test]
fn unreachable_store_is_an_error_not_a_missing_object() {
    let bucket = SharedBucket::default();
    let store = S3Store::new(bucket.clone(), "");
    bucket.fail(true);
    let hash = ContentHash::of(b"x");
    assert!(matches!(store.contains(&hash), Err(StoreError::Backend(_))));
}

#[test]
fn unsafe_ref_names_are_rejected() {
    let mut store = S3Store::new(SharedBucket::default(), "");
    let hash = ContentHash::of(b"x");
    assert!(store.write_ref("../escape", hash).is_err());
    assert!(store.compare_and_swap_ref("a//b", None, hash).is_err());
}
