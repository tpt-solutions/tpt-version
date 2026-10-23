//! Randomized commit histories driven through `Repository`.
//!
//! Two branches receive random commits from a shared fork point, then `dev` is
//! merged into `main`. For every seed the repository must agree with the branch
//! heads: clean merges commit with both heads as parents, conflicts block the
//! commit until each one is resolved, and a resolution lands the chosen value.
//! No commit may be lost from history.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{Rng, Vertex, author, id, new_repo, vertex};
use tpt_version::{Entity, LogicalClock, MemoryStore, RepoError, Repository, Resolution, dag};

type Repo = Repository<MemoryStore, LogicalClock>;
type Snapshot = BTreeMap<u8, Option<Vertex>>;

const ENTITIES: u8 = 6;
const SEEDS: u64 = 300;

/// Applies random edits to the working copy: removals, and single-axis moves.
fn edit(rng: &mut Rng, repo: &mut Repo) {
    for n in 0..ENTITIES {
        match rng.below(8) {
            0 => {
                repo.remove_entity(&id(n)).unwrap();
            }
            1..=3 => {
                let mut position = match repo.entity::<Vertex>(&id(n)).unwrap() {
                    Some(existing) => existing.position,
                    None => [0.0; 3],
                };
                let axis = rng.below(3) as usize;
                position[axis] = rng.below(4) as f64;
                repo.put_entity(&vertex(n, position)).unwrap();
            }
            _ => {}
        }
    }
}

/// Makes one to three commits on the current branch, labelled `label-i`.
fn commits(rng: &mut Rng, repo: &mut Repo, label: &str) {
    for i in 0..1 + rng.below(3) {
        edit(rng, repo);
        repo.commit(&format!("{label}-{i}"), &author()).unwrap();
    }
}

fn snapshot(repo: &Repo) -> Snapshot {
    (0..ENTITIES)
        .map(|n| (n, repo.entity::<Vertex>(&id(n)).unwrap()))
        .collect()
}

/// Runs one history. Returns whether the merge produced conflicts.
fn check(seed: u64) -> bool {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut repo = new_repo();

    commits(&mut rng, &mut repo, "base");
    let fork = repo.head().unwrap();
    repo.branch("dev", fork).unwrap();

    commits(&mut rng, &mut repo, "main");
    let main_head = repo.head().unwrap();
    let main_snap = snapshot(&repo);

    repo.switch_branch("dev").unwrap();
    commits(&mut rng, &mut repo, "dev");
    let dev_head = repo.head().unwrap();
    let dev_snap = snapshot(&repo);

    repo.switch_branch("main").unwrap();
    let result = repo.merge("dev").unwrap();

    let reported: BTreeSet<u8> = result
        .conflicts
        .iter()
        .map(|c| c.entity.as_bytes()[0])
        .collect();
    assert_eq!(
        repo.conflicts().len(),
        reported.len(),
        "seed {seed}: repository and merge result disagree on conflicts"
    );

    // Entities both sides left alone must come through unchanged.
    let untouched = |n: &u8| !reported.contains(n) && main_snap[n] == dev_snap[n];

    if reported.is_empty() {
        repo.commit("merge dev", &author()).unwrap();
    } else {
        match repo.commit("merge dev", &author()) {
            Err(RepoError::UnresolvedConflicts(count)) => assert_eq!(count, reported.len()),
            other => panic!("seed {seed}: commit with conflicts gave {other:?}"),
        }
        for conflict in repo.conflicts().to_vec() {
            let n = conflict.entity.as_bytes()[0];
            let (resolution, expected) = match rng.below(3) {
                0 => (Resolution::Ours, main_snap[&n].clone()),
                1 => (Resolution::Theirs, dev_snap[&n].clone()),
                _ => {
                    let custom = vertex(n, [9.0; 3]);
                    (Resolution::Custom(custom.to_uir()), Some(custom))
                }
            };
            repo.resolve_conflict(&conflict.entity, resolution).unwrap();
            assert_eq!(
                repo.entity::<Vertex>(&conflict.entity).unwrap(),
                expected,
                "seed {seed}: wrong value after resolving entity {n}"
            );
        }
        assert!(repo.conflicts().is_empty(), "seed {seed}: conflicts left");
        repo.commit("merge dev", &author()).unwrap();
    }

    let head = repo.head().unwrap();
    let merge_commit = dag::load_commit(repo.store(), &head).unwrap();
    assert_eq!(
        merge_commit.parents,
        [main_head, dev_head],
        "seed {seed}: merge commit must have both heads as parents"
    );

    for n in 0..ENTITIES {
        if untouched(&n) {
            let merged = repo.entity::<Vertex>(&id(n)).unwrap();
            assert_eq!(merged, main_snap[&n], "seed {seed}: entity {n} changed");
        }
    }

    // No commit is lost: every message from both branches is in history.
    let messages: BTreeSet<String> = repo
        .log("main")
        .unwrap()
        .into_iter()
        .map(|c| c.message)
        .collect();
    for label in ["base", "main", "dev"] {
        let found = messages.iter().filter(|m| m.starts_with(label)).count();
        assert!(found > 0, "seed {seed}: no {label} commit in history");
    }
    assert!(
        messages.contains("merge dev"),
        "seed {seed}: merge commit missing"
    );

    !reported.is_empty()
}

#[test]
fn random_histories_stay_consistent_through_the_repository() {
    let mut conflicting = 0;
    for seed in 1..=SEEDS {
        if check(seed) {
            conflicting += 1;
        }
    }
    // Both outcomes must be exercised, or the test proves less than it claims.
    assert!(conflicting > 0, "no run produced a conflict");
    assert!(conflicting < SEEDS, "every run produced a conflict");
}
