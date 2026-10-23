//! Rewriting and replaying history: amend, reset_hard, cherry_pick and revert.

mod common;

use common::{Vertex, author, id, new_repo, vertex};
use tpt_version::{RepoError, Resolution};

fn position(
    repo: &tpt_version::Repository<tpt_version::MemoryStore, tpt_version::LogicalClock>,
    n: u8,
) -> Option<[f64; 3]> {
    repo.entity::<Vertex>(&id(n))
        .unwrap()
        .map(|vertex| vertex.position)
}

#[test]
fn amend_replaces_the_head_and_keeps_its_parents() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    repo.commit("draft", &author()).unwrap();

    repo.put_entity(&vertex(2, [4.0; 3])).unwrap();
    let amended = repo.amend("final", &author()).unwrap();

    assert_eq!(repo.head(), Some(amended));
    let messages: Vec<String> = repo
        .log("main")
        .unwrap()
        .into_iter()
        .map(|c| c.message)
        .collect();
    assert_eq!(messages, ["final", "base"]);
    assert_eq!(repo.log("main").unwrap()[0].parents, [base]);
    assert_eq!(position(&repo, 2), Some([4.0; 3]));
}

#[test]
fn reset_hard_moves_the_branch_and_drops_uncommitted_work() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let first = repo.commit("first", &author()).unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    repo.commit("second", &author()).unwrap();
    repo.put_entity(&vertex(3, [1.0; 3])).unwrap();

    repo.reset_hard(first).unwrap();
    assert_eq!(repo.resolve_branch("main").unwrap(), Some(first));
    assert_eq!(repo.head(), Some(first));
    assert!(repo.status().unwrap().changes.is_empty());
    assert_eq!(position(&repo, 2), None);
    assert_eq!(position(&repo, 3), None);
}

#[test]
fn cherry_pick_applies_one_commit_onto_the_head() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(2, [2.0; 3])).unwrap();
    let wanted = repo.commit("wanted", &author()).unwrap();
    repo.put_entity(&vertex(4, [9.0; 3])).unwrap();
    repo.commit("not wanted", &author()).unwrap();

    repo.switch_branch("main").unwrap();
    repo.put_entity(&vertex(3, [3.0; 3])).unwrap();
    repo.commit("main work", &author()).unwrap();

    let picked = repo.cherry_pick(wanted).unwrap();
    assert!(picked.conflicts.is_empty());
    repo.commit("pick wanted", &author()).unwrap();
    assert_eq!(position(&repo, 2), Some([2.0; 3]));
    assert_eq!(position(&repo, 3), Some([3.0; 3]));
    assert_eq!(position(&repo, 4), None);
}

#[test]
fn revert_undoes_a_commit_and_keeps_later_work() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("base", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let mistake = repo.commit("mistake", &author()).unwrap();
    repo.put_entity(&vertex(2, [5.0; 3])).unwrap();
    repo.commit("later work", &author()).unwrap();

    let reverted = repo.revert(mistake).unwrap();
    assert!(reverted.conflicts.is_empty());
    repo.commit("revert mistake", &author()).unwrap();
    assert_eq!(position(&repo, 1), Some([0.0; 3]));
    assert_eq!(position(&repo, 2), Some([5.0; 3]));
}

#[test]
fn revert_that_clashes_with_a_later_edit_stages_a_conflict() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("base", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let mistake = repo.commit("mistake", &author()).unwrap();
    repo.put_entity(&vertex(1, [2.0, 0.0, 0.0])).unwrap();
    repo.commit("someone builds on it", &author()).unwrap();

    let reverted = repo.revert(mistake).unwrap();
    assert_eq!(reverted.conflicts.len(), 1);
    assert_eq!(
        repo.commit("too early", &author()),
        Err(RepoError::UnresolvedConflicts(1))
    );
    repo.resolve_conflict(&id(1), Resolution::Ours).unwrap();
    repo.commit("revert with ours", &author()).unwrap();
    assert_eq!(position(&repo, 1), Some([2.0, 0.0, 0.0]));
}
