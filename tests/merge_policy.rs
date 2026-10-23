//! Per-field merge strategies. Numeric fields that both sides changed can be
//! summed or clamped, and set-like lists can be unioned, instead of conflicting.
//! Fields without a strategy still conflict.

use tpt_version::merge::{FieldStrategy, ListStrategy, MergePolicy};
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

#[derive(Entity, Debug, Clone, PartialEq)]
struct Tagged {
    #[entity(id)]
    id: EntityId,
    tags: Vec<String>,
}

fn tagged(tags: &[&str]) -> Tagged {
    Tagged {
        id: EntityId::from_bytes([2; 16]),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
    }
}

/// Main drops `a` and adds `c`. The side keeps `a` and adds `d`.
fn tag_branches(policy: MergePolicy) -> Repository<MemoryStore, LogicalClock> {
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_merge_policy(policy);
    repo.put_entity(&tagged(&["a", "b"])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.put_entity(&tagged(&["b", "c"])).unwrap();
    repo.commit("main tags", &author()).unwrap();

    repo.switch_branch("side").unwrap();
    repo.put_entity(&tagged(&["a", "b", "d"])).unwrap();
    repo.commit("side tags", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo
}

#[test]
fn union_keeps_additions_and_lets_a_removal_win() {
    let policy = MergePolicy::new().list(&["tags"], ListStrategy::Union);
    let mut repo = tag_branches(policy);

    let result = repo.merge("side").unwrap();
    assert!(result.conflicts.is_empty());
    repo.commit("merge side", &author()).unwrap();
    let merged: Tagged = repo
        .entity(&EntityId::from_bytes([2; 16]))
        .unwrap()
        .unwrap();
    assert_eq!(merged.tags, ["b", "c", "d"]);
}

#[test]
fn without_a_list_strategy_the_list_conflicts() {
    let mut repo = tag_branches(MergePolicy::new());
    let result = repo.merge("side").unwrap();
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].fields[0].path, ["tags"]);
}

#[derive(Entity, Debug, Clone, PartialEq)]
struct Note {
    #[entity(id)]
    id: EntityId,
    body: String,
}

/// Main edits the first line of the body and the side edits the last line.
fn note_branches(policy: MergePolicy) -> Repository<MemoryStore, LogicalClock> {
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_merge_policy(policy);
    let note = |body: &str| Note {
        id: EntityId::from_bytes([5; 16]),
        body: body.into(),
    };
    repo.put_entity(&note("intro\nmiddle\nend")).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.put_entity(&note("INTRO\nmiddle\nend")).unwrap();
    repo.commit("main edits the start", &author()).unwrap();

    repo.switch_branch("side").unwrap();
    repo.put_entity(&note("intro\nmiddle\nFINISH")).unwrap();
    repo.commit("side edits the end", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo
}

#[test]
fn text_lines_merges_edits_to_different_lines() {
    let policy = MergePolicy::new().field(&["body"], FieldStrategy::TextLines);
    let mut repo = note_branches(policy);

    assert!(repo.merge("side").unwrap().conflicts.is_empty());
    repo.commit("merge side", &author()).unwrap();
    let merged: Note = repo
        .entity(&EntityId::from_bytes([5; 16]))
        .unwrap()
        .unwrap();
    assert_eq!(merged.body, "INTRO\nmiddle\nFINISH");
}

#[test]
fn text_without_a_text_strategy_still_conflicts() {
    let mut repo = note_branches(MergePolicy::new());
    assert_eq!(repo.merge("side").unwrap().conflicts.len(), 1);
}

/// Both sides change the body. `main_first` decides which commit is older.
fn lww_branches(policy: MergePolicy, main_first: bool) -> Repository<MemoryStore, LogicalClock> {
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_merge_policy(policy);
    let note = |body: &str| Note {
        id: EntityId::from_bytes([6; 16]),
        body: body.into(),
    };
    repo.put_entity(&note("base")).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    let main_edit = |repo: &mut Repository<MemoryStore, LogicalClock>| {
        repo.switch_branch("main").unwrap();
        repo.put_entity(&note("from main")).unwrap();
        repo.commit("main edit", &author()).unwrap();
    };
    let side_edit = |repo: &mut Repository<MemoryStore, LogicalClock>| {
        repo.switch_branch("side").unwrap();
        repo.put_entity(&note("from side")).unwrap();
        repo.commit("side edit", &author()).unwrap();
    };
    if main_first {
        main_edit(&mut repo);
        side_edit(&mut repo);
    } else {
        side_edit(&mut repo);
        main_edit(&mut repo);
    }
    repo.switch_branch("main").unwrap();
    repo
}

#[test]
fn last_writer_wins_takes_the_newer_commit() {
    let policy = MergePolicy::new().field(&["body"], FieldStrategy::LastWriterWins);
    let key = EntityId::from_bytes([6; 16]);

    // Main commits first, so the side's commit is the newer one.
    let mut side_newer = lww_branches(policy.clone(), true);
    assert!(side_newer.merge("side").unwrap().conflicts.is_empty());
    side_newer.commit("merge", &author()).unwrap();
    let kept: Note = side_newer.entity(&key).unwrap().unwrap();
    assert_eq!(kept.body, "from side");

    // The side commits first, so main's commit is the newer one.
    let mut main_newer = lww_branches(policy, false);
    assert!(main_newer.merge("side").unwrap().conflicts.is_empty());
    main_newer.commit("merge", &author()).unwrap();
    let kept: Note = main_newer.entity(&key).unwrap().unwrap();
    assert_eq!(kept.body, "from main");
}
