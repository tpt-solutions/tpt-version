//! Randomized three-way merges checked against an independent oracle.
//!
//! The oracle restates the merge rules over a plain model of entities and
//! integer fields. For every seed, the real merge must report exactly the
//! conflicts the oracle expects, must match it on every clean entity, and must
//! give the same answer whichever side is "ours".

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{id, put};
use tpt_version::merge::merge_trees;
use tpt_version::object::Object;
use tpt_version::store::CasStore;
use tpt_version::{ContentHash, MemoryStore, Scalar, Tree, UirNode};

type Fields = BTreeMap<String, i64>;
type Model = BTreeMap<u8, Fields>;

const ENTITIES: u8 = 6;
const FIELDS: [&str; 3] = ["x", "y", "z"];
const SEEDS: u64 = 400;

/// A small xorshift generator, so runs are reproducible without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

fn random_base(rng: &mut Rng) -> Model {
    let mut model = Model::new();
    for n in 0..ENTITIES {
        if rng.below(4) == 0 {
            continue;
        }
        let mut fields = Fields::new();
        for name in FIELDS {
            if rng.below(3) != 0 {
                fields.insert(name.to_owned(), rng.below(4) as i64);
            }
        }
        model.insert(n, fields);
    }
    model
}

/// Applies a few random edits: deletes, field changes, field removals and re-adds.
fn mutate(rng: &mut Rng, base: &Model) -> Model {
    let mut model = base.clone();
    for n in 0..ENTITIES {
        match rng.below(10) {
            0 => {
                model.remove(&n);
            }
            1..=4 => {
                let fields = model.entry(n).or_default();
                let name = FIELDS[rng.below(FIELDS.len() as u64) as usize];
                fields.insert(name.to_owned(), rng.below(4) as i64);
            }
            5 => {
                if let Some(fields) = model.get_mut(&n) {
                    let name = FIELDS[rng.below(FIELDS.len() as u64) as usize];
                    fields.remove(name);
                }
            }
            _ => {}
        }
    }
    model
}

fn build(store: &mut MemoryStore, model: &Model) -> ContentHash {
    let entries = model
        .iter()
        .map(|(n, fields)| {
            let node = UirNode::Map(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), UirNode::from(*value)))
                    .collect(),
            );
            (id(*n), put(store, &Object::Entity(node)))
        })
        .collect();
    put(store, &Object::Tree(Tree { entries }))
}

/// Reads a merged tree back into a model. Conflict entries are returned separately.
fn read(store: &MemoryStore, tree: &ContentHash) -> (Model, BTreeSet<u8>) {
    let Object::Tree(tree) = Object::decode(&store.get(tree).unwrap()).unwrap() else {
        panic!("not a tree");
    };
    let mut model = Model::new();
    let mut conflicted = BTreeSet::new();
    for (entity, hash) in tree.entries {
        let n = entity.as_bytes()[0];
        match Object::decode(&store.get(&hash).unwrap()).unwrap() {
            Object::Entity(UirNode::Map(map)) => {
                let fields = map
                    .into_iter()
                    .map(|(name, value)| match value {
                        UirNode::Scalar(Scalar::Int(v)) => (name, v),
                        other => panic!("unexpected field value {other:?}"),
                    })
                    .collect();
                model.insert(n, fields);
            }
            Object::Conflict(_) => {
                conflicted.insert(n);
            }
            other => panic!("unexpected object {}", other.kind()),
        }
    }
    (model, conflicted)
}

/// The oracle. `Ok` gives the clean result, `Err` means the entity conflicts.
fn expected(
    base: Option<&Fields>,
    ours: Option<&Fields>,
    theirs: Option<&Fields>,
) -> Result<Option<Fields>, ()> {
    if ours == theirs {
        return Ok(ours.cloned());
    }
    if base == ours {
        return Ok(theirs.cloned());
    }
    if base == theirs {
        return Ok(ours.cloned());
    }
    let (Some(ours), Some(theirs)) = (ours, theirs) else {
        return Err(());
    };
    let empty = Fields::new();
    let base = base.unwrap_or(&empty);
    let mut merged = Fields::new();
    let names: BTreeSet<&String> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .collect();
    for name in names {
        let (b, o, t) = (base.get(name), ours.get(name), theirs.get(name));
        let value = if o == t {
            o
        } else if b == o {
            t
        } else if b == t {
            o
        } else {
            return Err(());
        };
        if let Some(value) = value {
            merged.insert(name.clone(), *value);
        }
    }
    Ok(Some(merged))
}

fn check(seed: u64, store: &mut MemoryStore, base: &Model, ours: &Model, theirs: &Model) {
    let base_hash = build(store, base);
    let ours_hash = build(store, ours);
    let theirs_hash = build(store, theirs);

    let forward = merge_trees(store, base_hash, ours_hash, theirs_hash).unwrap();
    let backward = merge_trees(store, base_hash, theirs_hash, ours_hash).unwrap();

    let reported: BTreeSet<u8> = forward
        .conflicts
        .iter()
        .map(|c| c.entity.as_bytes()[0])
        .collect();
    let reported_back: BTreeSet<u8> = backward
        .conflicts
        .iter()
        .map(|c| c.entity.as_bytes()[0])
        .collect();
    assert_eq!(
        reported, reported_back,
        "seed {seed}: conflicts depend on side order"
    );

    let ids: BTreeSet<u8> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .copied()
        .collect();
    let mut expected_conflicts = BTreeSet::new();
    let mut expected_values = BTreeMap::new();
    for n in ids {
        match expected(base.get(&n), ours.get(&n), theirs.get(&n)) {
            Ok(value) => {
                expected_values.insert(n, value);
            }
            Err(()) => {
                expected_conflicts.insert(n);
            }
        }
    }
    assert_eq!(
        reported, expected_conflicts,
        "seed {seed}: wrong conflict set"
    );

    let (merged, conflicted) = read(store, &forward.merged_tree);
    assert_eq!(
        conflicted, reported,
        "seed {seed}: conflict objects disagree with report"
    );
    for (n, value) in &expected_values {
        assert_eq!(merged.get(n), value.as_ref(), "seed {seed}: entity {n}");
    }

    if reported.is_empty() {
        assert_eq!(
            forward.merged_tree, backward.merged_tree,
            "seed {seed}: merge must commute"
        );
    }
}

#[test]
fn random_three_way_merges_match_the_oracle() {
    let mut store = MemoryStore::default();
    let mut divergent_runs = 0;
    for seed in 1..=SEEDS {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let base = random_base(&mut rng);
        let ours = mutate(&mut rng, &base);
        let theirs = mutate(&mut rng, &base);
        if !base.is_empty() && ours != theirs {
            divergent_runs += 1;
        }
        check(seed, &mut store, &base, &ours, &theirs);
    }
    // Make sure the generator actually exercises divergent edits.
    assert!(
        divergent_runs > SEEDS / 2,
        "only {divergent_runs} divergent runs"
    );
}
