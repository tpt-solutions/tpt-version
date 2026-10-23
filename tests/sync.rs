//! Moving branches between stores with push and fetch.

mod common;

use common::{author, new_repo, vertex};
use tpt_version::sync::{fetch, push};
use tpt_version::{LogicalClock, MemoryStore, RepoError, Repository};

fn with_two_commits() -> Repository<MemoryStore, LogicalClock> {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("one", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    repo.commit("two", &author()).unwrap();
    repo
}

#[test]
fn push_copies_the_history_and_repeats_as_a_no_op() {
    let local = with_two_commits();
    let mut remote = MemoryStore::default();

    let first = push(local.store(), &mut remote, "main").unwrap();
    assert!(first.updated);
    assert!(first.objects_copied > 0);
    assert_eq!(Some(first.head), local.head());

    let reopened = Repository::open(remote.clone(), LogicalClock::starting_at(1), "main").unwrap();
    assert_eq!(reopened.log("main").unwrap().len(), 2);

    let again = push(local.store(), &mut remote, "main").unwrap();
    assert!(!again.updated);
    assert_eq!(again.objects_copied, 0);
}

#[test]
fn push_refuses_to_overwrite_commits_the_remote_has() {
    let local = with_two_commits();
    let mut other = new_repo();
    other.put_entity(&vertex(9, [9.0; 3])).unwrap();
    other.commit("someone else", &author()).unwrap();
    let mut remote = other.store().clone();

    assert_eq!(
        push(local.store(), &mut remote, "main"),
        Err(RepoError::NotFastForward("main".into()))
    );
}

#[test]
fn fetch_brings_a_remote_branch_into_an_empty_store() {
    let remote = with_two_commits();
    let mut local = MemoryStore::default();

    let fetched = fetch(remote.store(), &mut local, "main").unwrap();
    assert!(fetched.updated);

    let repo = Repository::open(local, LogicalClock::starting_at(1), "main").unwrap();
    assert_eq!(repo.head(), remote.head());
    assert_eq!(repo.log("main").unwrap().len(), 2);
}

#[test]
fn a_branch_missing_on_the_source_is_an_error() {
    let local = with_two_commits();
    let mut remote = MemoryStore::default();
    assert_eq!(
        push(local.store(), &mut remote, "nope"),
        Err(RepoError::BranchNotFound("nope".into()))
    );
}
