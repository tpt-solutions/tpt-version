//! Per-entity history and field-level blame on the current branch.

mod common;

use common::{author, id, new_repo, vertex};
use tpt_version::ContentHash;
use tpt_version::history::{EntityVersion, FieldBlame};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

#[test]
fn history_lists_only_the_commits_that_changed_the_entity() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let c1 = repo.commit("one", &author()).unwrap();
    repo.put_entity(&vertex(9, [0.0; 3])).unwrap();
    repo.commit("unrelated", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let c3 = repo.commit("x moves", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 2.0, 0.0])).unwrap();
    let c4 = repo.commit("y moves", &author()).unwrap();

    let commits: Vec<ContentHash> = repo
        .entity_history(&id(1))
        .unwrap()
        .iter()
        .map(|version| version.commit)
        .collect();
    assert_eq!(commits, [c4, c3, c1]);
}

#[test]
fn blame_names_the_commit_that_set_each_field() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let c1 = repo.commit("one", &author()).unwrap();
    repo.put_entity(&vertex(9, [0.0; 3])).unwrap();
    repo.commit("unrelated", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let c3 = repo.commit("x moves", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 2.0, 0.0])).unwrap();
    let c4 = repo.commit("y moves", &author()).unwrap();

    let blame = repo.field_blame(&id(1)).unwrap();
    assert_eq!(blame.len(), 4);
    for (field, commit) in [
        (path(&["id"]), c1),
        (path(&["position", "x"]), c3),
        (path(&["position", "y"]), c4),
        (path(&["position", "z"]), c1),
    ] {
        assert!(
            blame.contains(&FieldBlame {
                path: field.clone(),
                commit
            }),
            "{field:?} should be blamed on {commit}"
        );
    }
}

#[test]
fn removal_ends_the_history_and_leaves_no_blame() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("one", &author()).unwrap();
    repo.remove_entity(&id(1)).unwrap();
    let removed = repo.commit("remove", &author()).unwrap();

    let history = repo.entity_history(&id(1)).unwrap();
    assert_eq!(
        history.first(),
        Some(&EntityVersion {
            commit: removed,
            content: None,
        })
    );
    assert!(repo.field_blame(&id(1)).unwrap().is_empty());
}

#[test]
fn entity_at_reads_a_past_commit_not_the_working_copy() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let before = repo.commit("before", &author()).unwrap();
    repo.put_entity(&vertex(1, [7.0; 3])).unwrap();
    repo.commit("after", &author()).unwrap();
    repo.put_entity(&vertex(1, [9.0; 3])).unwrap();

    let then: common::Vertex = repo.entity_at(&before, &id(1)).unwrap().unwrap();
    assert_eq!(then.position, [0.0; 3]);
    let missing: Option<common::Vertex> = repo.entity_at(&before, &id(5)).unwrap();
    assert!(missing.is_none());
}
