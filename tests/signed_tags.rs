//! Annotated tags: a message and an optional signature, under the tags/ refs.

mod common;

use common::{author, new_repo, vertex};
use tpt_version::maintenance::fsck;
use tpt_version::sign::Ed25519Signer;
use tpt_version::{ContentHash, LogicalClock, MemoryStore, RepoError, Repository};

fn signed_repo() -> Repository<MemoryStore, LogicalClock> {
    Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_signer(Box::new(Ed25519Signer::from_seed([7; 32])))
}

#[test]
fn signed_annotated_tag_resolves_to_its_commit_and_verifies() {
    let mut repo = signed_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("release", &author()).unwrap();
    repo.create_annotated_tag("v1.0", commit, "first release", &author())
        .unwrap();

    assert_eq!(repo.resolve_tag("v1.0").unwrap(), Some(commit));
    let tag = repo.annotated_tag("v1.0").unwrap().expect("annotated");
    assert_eq!(tag.message, "first release");
    assert_eq!(tag.name, "v1.0");
    assert!(tag.is_valid());
    assert_eq!(repo.list_tags().unwrap(), ["v1.0"]);
}

#[test]
fn an_altered_tag_message_fails_verification() {
    let mut repo = signed_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("release", &author()).unwrap();
    repo.create_annotated_tag("v1.0", commit, "first release", &author())
        .unwrap();
    let mut tag = repo.annotated_tag("v1.0").unwrap().unwrap();
    tag.message = "shipped the wrong build".into();
    assert!(!tag.is_valid());
}

#[test]
fn unsigned_annotated_tag_is_not_valid() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("release", &author()).unwrap();
    repo.create_annotated_tag("v0.9", commit, "beta", &author())
        .unwrap();
    assert!(!repo.annotated_tag("v0.9").unwrap().unwrap().is_valid());
}

#[test]
fn lightweight_tags_have_no_annotation_and_names_stay_unique() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("release", &author()).unwrap();
    repo.create_tag("light", commit).unwrap();
    assert!(repo.annotated_tag("light").unwrap().is_none());
    assert_eq!(repo.resolve_tag("light").unwrap(), Some(commit));

    repo.create_annotated_tag("v1", commit, "one", &author())
        .unwrap();
    assert_eq!(
        repo.create_annotated_tag("v1", commit, "again", &author()),
        Err(RepoError::TagExists("v1".into()))
    );
    assert_eq!(
        repo.create_tag("v1", commit),
        Err(RepoError::TagExists("v1".into()))
    );
}

#[test]
fn an_annotated_tag_for_an_unknown_commit_is_refused() {
    let mut repo = new_repo();
    assert!(
        repo.create_annotated_tag("ghost", ContentHash::of(b"nothing"), "", &author())
            .is_err()
    );
}

#[test]
fn a_tagged_commit_survives_gc_and_fsck_is_clean() {
    let mut repo = signed_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let kept = repo.commit("kept", &author()).unwrap();
    repo.branch("temp", kept).unwrap();
    repo.switch_branch("temp").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    let tagged = repo.commit("tagged", &author()).unwrap();
    repo.create_annotated_tag("keep-me", tagged, "keep", &author())
        .unwrap();
    repo.switch_branch("main").unwrap();
    repo.delete_branch("temp").unwrap();

    assert_eq!(repo.gc().unwrap().removed, 0);
    assert!(fsck(repo.store()).unwrap().is_clean());
    assert_eq!(repo.resolve_tag("keep-me").unwrap(), Some(tagged));
}
