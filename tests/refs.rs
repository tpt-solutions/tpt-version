//! Compare-and-swap on branch refs: stale writers are refused, and concurrent
//! writers to one ref form a single unbroken chain.
#![cfg(feature = "std")]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use common::{author, vertex};
use tpt_version::store::FileSystemStore;
use tpt_version::store::RefStore;
use tpt_version::{ContentHash, LogicalClock, MemoryStore, RepoError, Repository, StoreError};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpt-version-refs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn memory_cas_only_moves_from_the_expected_value() {
    let mut store = MemoryStore::default();
    let first = ContentHash::of(b"first");
    let second = ContentHash::of(b"second");

    assert!(store.compare_and_swap_ref("main", None, first).unwrap());
    assert!(!store.compare_and_swap_ref("main", None, second).unwrap());
    assert_eq!(store.read_ref("main").unwrap(), Some(first));
    assert!(
        store
            .compare_and_swap_ref("main", Some(first), second)
            .unwrap()
    );
    assert_eq!(store.read_ref("main").unwrap(), Some(second));
}

#[test]
fn stale_repository_is_refused_and_ref_is_untouched() {
    let root = scratch_dir("stale");
    let mut a = Repository::open(
        FileSystemStore::open(&root).unwrap(),
        LogicalClock::starting_at(1),
        "main",
    )
    .unwrap();
    // Opened while `main` is still unborn, so it expects no ref.
    let mut b = Repository::open(
        FileSystemStore::open(&root).unwrap(),
        LogicalClock::starting_at(1),
        "main",
    )
    .unwrap();

    a.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let winner = a.commit("from a", &author()).unwrap();

    b.put_entity(&vertex(2, [1.0; 3])).unwrap();
    let err = b.commit("from b", &author()).unwrap_err();
    assert_eq!(err, RepoError::RefMoved("main".into()));

    let reader = FileSystemStore::open(&root).unwrap();
    assert_eq!(reader.read_ref("main").unwrap(), Some(winner));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn held_lock_refuses_the_update_and_leaves_no_lock_behind() {
    let root = scratch_dir("locked");
    let mut store = FileSystemStore::open(&root).unwrap();
    let hash = ContentHash::of(b"commit");
    let lock = root.join("refs").join("heads").join("main.lock");
    fs::write(&lock, b"").unwrap();

    let err = store.compare_and_swap_ref("main", None, hash).unwrap_err();
    assert!(matches!(err, StoreError::Backend(message) if message.contains("locked")));
    assert!(
        lock.exists(),
        "a refused writer must not remove someone else's lock"
    );

    fs::remove_file(&lock).unwrap();
    assert!(store.compare_and_swap_ref("main", None, hash).unwrap());
    assert!(
        !lock.exists(),
        "the lock is released after a successful swap"
    );
    assert_eq!(store.list_refs().unwrap(), ["main"]);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn concurrent_swaps_form_a_single_chain() {
    const THREADS: u8 = 8;
    const ATTEMPTS: u8 = 25;

    let root = scratch_dir("chain");
    FileSystemStore::open(&root).unwrap();
    let successes = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<_> = (0..THREADS)
        .map(|thread_id| {
            let root = root.clone();
            let successes = Arc::clone(&successes);
            thread::spawn(move || {
                let mut store = FileSystemStore::open(&root).unwrap();
                for attempt in 0..ATTEMPTS {
                    let current = store.read_ref("main").unwrap();
                    let next = ContentHash::of(&[thread_id, attempt]);
                    // A held lock is a transient refusal, so retry the same step.
                    match store.compare_and_swap_ref("main", current, next) {
                        Ok(true) => successes.lock().unwrap().push((current, next)),
                        Ok(false) | Err(StoreError::Backend(_)) => {}
                        Err(other) => panic!("unexpected error: {other}"),
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    // Every successful swap must have started from the value the previous
    // successful swap produced, so the pairs form one chain from `None`.
    let pairs = successes.lock().unwrap().clone();
    let mut next_of: BTreeMap<Option<ContentHash>, ContentHash> = BTreeMap::new();
    for (from, to) in &pairs {
        assert!(
            next_of.insert(*from, *to).is_none(),
            "two swaps started from {from:?}"
        );
    }
    let store = FileSystemStore::open(&root).unwrap();
    let mut cursor = None;
    let mut walked = 0;
    while let Some(next) = next_of.get(&cursor) {
        walked += 1;
        cursor = Some(*next);
    }
    assert_eq!(walked, pairs.len(), "swaps do not form one chain");
    assert_eq!(store.read_ref("main").unwrap(), cursor);
    fs::remove_dir_all(&root).unwrap();
}
