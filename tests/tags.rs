//! Tags: immutable names for commits, kept apart from branches, and treated as
//! roots by gc and fsck. Also the status summary.

mod common;

use common::{author, id, new_repo, vertex};
use tpt_version::RepoError;
use tpt_version::diff::DiffEntry;
use tpt_version::maintenance::fsck;

#[test]
fn tags_are_immutable_and_separate_from_branches() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let v1 = repo.commit("release 1", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    let v2 = repo.commit("release 2", &author()).unwrap();

    repo.create_tag("v1.0", v1).unwrap();
    assert_eq!(
        repo.create_tag("v1.0", v2),
        Err(RepoError::TagExists("v1.0".into()))
    );
    assert_eq!(repo.resolve_tag("v1.0").unwrap(), Some(v1));
    assert_eq!(repo.list_tags().unwrap(), ["v1.0"]);
    assert_eq!(repo.list_branches().unwrap(), ["main"]);

    // A tag name is not a branch name, and a branch cannot use the tag namespace.
    assert!(matches!(
        repo.branch("tags/v2", v2),
        Err(RepoError::InvalidBranchName(_))
    ));

    repo.delete_tag("v1.0").unwrap();
    assert_eq!(
        repo.delete_tag("v1.0"),
        Err(RepoError::TagNotFound("v1.0".into()))
    );
}

#[test]
fn a_tag_keeps_its_commit_through_gc() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let kept = repo.commit("kept", &author()).unwrap();
    repo.create_tag("kept", kept).unwrap();

    repo.branch("scratch", kept).unwrap();
    repo.switch_branch("scratch").unwrap();
    repo.put_entity(&vertex(2, [5.0; 3])).unwrap();
    let tagged = repo.commit("tagged scratch", &author()).unwrap();
    repo.create_tag("scratch-tag", tagged).unwrap();
    repo.switch_branch("main").unwrap();
    repo.delete_branch("scratch").unwrap();

    // The branch is gone, but the tag still holds its commit, tree and entity.
    let report = repo.gc().unwrap();
    assert_eq!(report.removed, 0);
    assert!(fsck(repo.store()).unwrap().is_clean());
    assert!(repo.entity::<common::Vertex>(&id(2)).unwrap().is_none());
}

#[test]
fn status_summarises_the_working_copy() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("base", &author()).unwrap();

    let clean = repo.status().unwrap();
    assert_eq!(clean.branch.as_deref(), Some("main"));
    assert_eq!(clean.head, Some(head));
    assert!(clean.changes.is_empty());
    assert!(clean.conflicts.is_empty());
    assert!(!clean.merging);

    repo.put_entity(&vertex(1, [2.0; 3])).unwrap();
    let dirty = repo.status().unwrap();
    assert_eq!(dirty.changes.len(), 1);
    assert!(matches!(dirty.changes[0], DiffEntry::Modified { .. }));
}
