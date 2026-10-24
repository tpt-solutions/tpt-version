//! Validators: commits are refused while a rule fails, and the rule can span
//! entities.

mod common;

use std::collections::BTreeMap;

use common::{Vertex, author, new_repo, vertex};
use tpt_version::{Entity, EntityId, RepoError, UirNode, Validator};

/// Every vertex must have a non-negative x coordinate.
struct NonNegativeX;

impl Validator for NonNegativeX {
    fn check(&self, entities: &BTreeMap<EntityId, UirNode>) -> Vec<String> {
        entities
            .values()
            .filter_map(|node| Vertex::from_uir(node).ok())
            .filter(|found| found.position[0] < 0.0)
            .map(|found| format!("vertex {} has negative x", found.id))
            .collect()
    }
}

/// No two vertices may share a position. This rule needs the whole snapshot.
struct UniquePositions;

impl Validator for UniquePositions {
    fn check(&self, entities: &BTreeMap<EntityId, UirNode>) -> Vec<String> {
        let mut seen = Vec::new();
        let mut problems = Vec::new();
        for node in entities.values() {
            let Ok(found) = Vertex::from_uir(node) else {
                continue;
            };
            let key = found.position.map(f64::to_bits);
            if seen.contains(&key) {
                problems.push(format!("vertex {} shares a position", found.id));
            }
            seen.push(key);
        }
        problems
    }
}

#[test]
fn commit_is_refused_until_the_rule_passes() {
    let mut repo = new_repo().with_validator(Box::new(NonNegativeX));
    repo.put_entity(&vertex(1, [-1.0, 0.0, 0.0])).unwrap();

    let refused = repo.commit("bad", &author());
    assert!(
        matches!(refused, Err(RepoError::ValidationFailed(ref problems)) if problems.len() == 1)
    );
    assert_eq!(repo.validate().unwrap().len(), 1);

    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    assert!(repo.validate().unwrap().is_empty());
    repo.commit("fixed", &author()).unwrap();
}

#[test]
fn a_rule_can_span_entities() {
    let mut repo = new_repo().with_validator(Box::new(UniquePositions));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    assert!(matches!(
        repo.commit("clash", &author()),
        Err(RepoError::ValidationFailed(_))
    ));

    repo.put_entity(&vertex(2, [5.0, 0.0, 0.0])).unwrap();
    repo.commit("apart", &author()).unwrap();
}

#[test]
fn a_merge_that_brings_in_a_bad_entity_is_refused_until_fixed() {
    // The bad vertex is committed on `alt` before the validator exists.
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("alt", base).unwrap();
    repo.switch_branch("alt").unwrap();
    repo.put_entity(&vertex(2, [-3.0, 0.0, 0.0])).unwrap();
    repo.commit("alt side", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    let mut repo = repo.with_validator(Box::new(NonNegativeX));
    repo.merge("alt").unwrap();
    assert!(matches!(
        repo.commit("merge", &author()),
        Err(RepoError::ValidationFailed(_))
    ));

    // The merge is still pending, so the fix goes into the same merge.
    repo.put_entity(&vertex(2, [3.0, 0.0, 0.0])).unwrap();
    let merged = repo.commit("merge", &author()).unwrap();
    assert_eq!(repo.log("main").unwrap().len(), 3);
    assert!(repo.resolve_branch("main").unwrap() == Some(merged));
}

#[test]
fn no_validators_means_no_checks() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [-1.0, 0.0, 0.0])).unwrap();
    repo.commit("unchecked", &author()).unwrap();
    assert!(repo.validate().unwrap().is_empty());
}
