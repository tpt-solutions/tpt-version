//! Per-field merge strategies: numeric fields that both sides changed can be
//! summed or clamped instead of conflicting, while other fields still conflict.

use tpt_version::merge::{FieldStrategy, MergePolicy};
use tpt_version::prelude::*;

#[derive(Entity, Debug, Clone, PartialEq)]
struct Counter {
    #[entity(id)]
    id: EntityId,
    hits: i64,
    peak: i64,
    label: String,
}

fn counter(hits: i64, peak: i64, label: &str) -> Counter {
    Counter {
        id: EntityId::from_bytes([1; 16]),
        hits,
        peak,
        label: label.into(),
    }
}

fn author() -> ActorId {
    ActorId::from_bytes([1; 16])
}

/// Main and a side branch both change the counter from the same base. The side
/// sets its label to `side_label`.
fn diverged(policy: MergePolicy, side_label: &str) -> Repository<MemoryStore, LogicalClock> {
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_merge_policy(policy);
    repo.put_entity(&counter(10, 10, "a")).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.put_entity(&counter(13, 15, "b")).unwrap();
    repo.commit("main work", &author()).unwrap();

    repo.switch_branch("side").unwrap();
    repo.put_entity(&counter(14, 12, side_label)).unwrap();
    repo.commit("side work", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo
}

#[test]
fn sum_and_max_combine_both_sides_and_other_fields_still_conflict() {
    let policy = MergePolicy::new()
        .field(&["hits"], FieldStrategy::Sum)
        .field(&["peak"], FieldStrategy::Max);
    // The side leaves the label alone, so only the numbers overlap.
    let mut repo = diverged(policy, "a");

    let result = repo.merge("side").unwrap();
    assert!(result.conflicts.is_empty());
    repo.commit("merge side", &author()).unwrap();
    let merged: Counter = repo
        .entity(&EntityId::from_bytes([1; 16]))
        .unwrap()
        .unwrap();
    // hits: 10 + (13 - 10) + (14 - 10) = 17. peak: max(15, 12) = 15.
    assert_eq!(merged.hits, 17);
    assert_eq!(merged.peak, 15);
    assert_eq!(merged.label, "b");
}

#[test]
fn without_a_policy_every_overlapping_field_conflicts() {
    let mut repo = diverged(MergePolicy::new(), "c");
    let result = repo.merge("side").unwrap();
    let mut fields: Vec<String> = result.conflicts[0]
        .fields
        .iter()
        .map(|field| field.path.join("."))
        .collect();
    fields.sort();
    assert_eq!(fields, ["hits", "label", "peak"]);
}

#[test]
fn min_keeps_the_smaller_value() {
    let policy = MergePolicy::new()
        .field(&["peak"], FieldStrategy::Min)
        .field(&["label"], FieldStrategy::Min);
    let mut repo = diverged(policy, "c");
    let result = repo.merge("side").unwrap();
    // The label is text, so the policy does not apply to it and it still conflicts.
    assert_eq!(result.conflicts.len(), 1);
    let peak_in_result = result.conflicts[0]
        .fields
        .iter()
        .any(|field| field.path == ["peak"]);
    assert!(!peak_in_result, "peak is combined, not conflicted");
}
