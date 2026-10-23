//! Two machines sync one repository through a shared bucket. The bucket here is
//! an in-memory stand-in. In a real program, `ObjectClient` and
//! `ConditionalClient` are implemented over your S3 SDK: `If-None-Match: *`
//! for a create, and `If-Match` with the read ETag for an update.
//!
//! Run with `cargo run --example s3_sync --features s3`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tpt_version::store::{ConditionalClient, ObjectClient, S3Store};
use tpt_version::{ActorId, LogicalClock, RepoError, Repository};

#[derive(Default)]
struct Bucket {
    /// Body and a version, which serves as the ETag.
    objects: BTreeMap<String, (Vec<u8>, u64)>,
    next_version: u64,
}

/// A cheap handle to one bucket, so each machine gets its own client.
#[derive(Clone, Default)]
struct Machine(Arc<Mutex<Bucket>>);

impl ObjectClient for Machine {
    fn put_object(&mut self, key: &str, body: &[u8]) -> Result<(), String> {
        let mut bucket = self.0.lock().map_err(|_| "poisoned")?;
        bucket.next_version += 1;
        let version = bucket.next_version;
        bucket.objects.insert(key.into(), (body.to_vec(), version));
        Ok(())
    }

    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let bucket = self.0.lock().map_err(|_| "poisoned")?;
        Ok(bucket.objects.get(key).map(|(body, _)| body.clone()))
    }

    fn head_object(&self, key: &str) -> Result<bool, String> {
        let bucket = self.0.lock().map_err(|_| "poisoned")?;
        Ok(bucket.objects.contains_key(key))
    }
}

impl ConditionalClient for Machine {
    fn get_object_with_etag(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, String> {
        let bucket = self.0.lock().map_err(|_| "poisoned")?;
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
        let mut bucket = self.0.lock().map_err(|_| "poisoned")?;
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
        let mut bucket = self.0.lock().map_err(|_| "poisoned")?;
        Ok(bucket.objects.remove(key).is_some())
    }

    fn list_keys(&self, prefix: &str) -> Result<Vec<String>, String> {
        let bucket = self.0.lock().map_err(|_| "poisoned")?;
        Ok(bucket
            .objects
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect())
    }
}

fn open(bucket: &Machine) -> Result<Repository<S3Store<Machine>, LogicalClock>, RepoError> {
    Repository::open(
        S3Store::new(bucket.clone(), "projects/bracket"),
        LogicalClock::starting_at(1),
        "main",
    )
}

fn main() -> Result<(), RepoError> {
    let bucket = Machine::default();
    let laptop_author = ActorId::from_bytes([1; 16]);
    let desk_author = ActorId::from_bytes([2; 16]);

    let mut laptop = open(&bucket)?;
    laptop.commit("laptop: first layout", &laptop_author)?;
    // The desk opens the same branch and starts from the laptop's commit.
    let mut desk = open(&bucket)?;
    desk.commit("desk: dimension change", &desk_author)?;

    // The laptop still expects the old head, so its push is refused.
    match laptop.commit("laptop: stale change", &laptop_author) {
        Err(RepoError::RefMoved(name)) => {
            println!("laptop refused: branch {name:?} moved on the bucket");
        }
        other => println!("unexpected: {other:?}"),
    }

    // Reopening picks up the desk's work, and the laptop can continue from it.
    let mut laptop = open(&bucket)?;
    println!("history on main after sync:");
    for commit in laptop.log("main")? {
        println!("  {}", commit.message);
    }
    laptop.commit("laptop: follow-up", &laptop_author)?;
    Ok(())
}
