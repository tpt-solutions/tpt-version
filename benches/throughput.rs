//! Benchmarks for the TODO's targets: committing 10,000 entities, diffing two
//! large trees, merging two divergent branches, and merging two long histories.
//!
//! Run with `cargo bench --bench throughput`. Criterion reports the spread of
//! each measurement. Setup work, such as building the branches, is excluded
//! from the timings.

use std::collections::BTreeMap;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tpt_version::diff::diff_commits;
use tpt_version::error::ObjectError;
use tpt_version::{
    ActorId, ContentHash, Entity, EntityId, LogicalClock, MemoryStore, Repository, Scalar, UirNode,
};

const ENTITIES: u32 = 10_000;
const AUTHOR: ActorId = ActorId::from_bytes([1; 16]);

type Repo = Repository<MemoryStore, LogicalClock>;

/// A minimal entity: an id and one integer value.
struct Cell {
    id: u32,
    value: i64,
}

impl Entity for Cell {
    fn id(&self) -> EntityId {
        let mut bytes = [0u8; 16];
        bytes[12..].copy_from_slice(&self.id.to_be_bytes());
        EntityId::from_bytes(bytes)
    }

    fn to_uir(&self) -> UirNode {
        UirNode::Map(BTreeMap::from([
            ("id".into(), UirNode::from(i64::from(self.id))),
            ("value".into(), UirNode::from(self.value)),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("cell"));
        };
        let int = |name| match fields.get(name) {
            Some(UirNode::Scalar(Scalar::Int(value))) => Ok(*value),
            _ => Err(ObjectError::InvalidValue("cell field")),
        };
        Ok(Self {
            id: u32::try_from(int("id")?).map_err(|_| ObjectError::InvalidValue("cell id"))?,
            value: int("value")?,
        })
    }
}

fn open(branch: &str) -> Repo {
    Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), branch).unwrap()
}

/// A committed base of 10,000 entities, and two branches that edit disjoint
/// halves of it. Returns the repository on `right`, with the base and `left`.
fn divergent_branches() -> (Repo, ContentHash, ContentHash) {
    let mut repo = open("main");
    for id in 0..ENTITIES {
        repo.put_entity(&Cell { id, value: 0 }).unwrap();
    }
    let base = repo.commit("base", &AUTHOR).unwrap();

    repo.branch("left", base).unwrap();
    repo.branch("right", base).unwrap();
    repo.switch_branch("left").unwrap();
    for id in (0..ENTITIES).step_by(2) {
        repo.put_entity(&Cell { id, value: 1 }).unwrap();
    }
    let left = repo.commit("edit evens", &AUTHOR).unwrap();
    repo.switch_branch("right").unwrap();
    for id in (1..ENTITIES).step_by(2) {
        repo.put_entity(&Cell { id, value: 2 }).unwrap();
    }
    repo.commit("edit odds", &AUTHOR).unwrap();
    (repo, base, left)
}

/// Two branches with long histories of their own, forked from a 2,000-commit
/// trunk. Returns the repository on `side`, which has 1,000 commits of its own.
fn long_histories() -> Repo {
    const TRUNK: u32 = 2_000;
    const SIDE: u32 = 1_000;

    let mut repo = open("trunk");
    let mut fork = None;
    for step in 0..TRUNK {
        repo.put_entity(&Cell {
            id: step % 16,
            value: i64::from(step),
        })
        .unwrap();
        let commit = repo.commit("trunk", &AUTHOR).unwrap();
        if step == TRUNK / 2 {
            fork = Some(commit);
        }
    }
    repo.branch("side", fork.expect("fork point is set"))
        .unwrap();
    repo.switch_branch("side").unwrap();
    for step in 0..SIDE {
        // Ids 100 and up, so the side branch never edits what the trunk edits.
        repo.put_entity(&Cell {
            id: 100 + step % 16,
            value: i64::from(step),
        })
        .unwrap();
        repo.commit("side", &AUTHOR).unwrap();
    }
    repo
}

fn commit_entities(c: &mut Criterion) {
    c.bench_function("commit 10,000 entities", |b| {
        b.iter(|| {
            let mut repo = open("main");
            for id in 0..ENTITIES {
                repo.put_entity(&Cell { id, value: 0 }).unwrap();
            }
            repo.commit("base", &AUTHOR).unwrap()
        });
    });
}

fn diff_trees(c: &mut Criterion) {
    let (repo, base, left) = divergent_branches();
    c.bench_function("diff two 10,000-entity trees", |b| {
        b.iter(|| diff_commits(repo.store(), &base, &left).unwrap().len());
    });
}

fn merge_branches(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge");
    group.sample_size(10);
    group.bench_function("two divergent branches of 10,000 entities", |b| {
        b.iter_batched(
            || {
                let (mut repo, _, _) = divergent_branches();
                repo.switch_branch("left").unwrap();
                repo
            },
            |mut repo| {
                let merged = repo.merge("right").unwrap();
                // Disjoint edits must not conflict.
                assert!(merged.conflicts.is_empty());
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("two histories of 2,000 and 1,000 commits", |b| {
        b.iter_batched(
            long_histories,
            |mut repo| {
                let merged = repo.merge("trunk").unwrap();
                assert!(merged.conflicts.is_empty());
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

criterion_group!(benches, commit_entities, diff_trees, merge_branches);
criterion_main!(benches);
