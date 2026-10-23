//! Wall-clock throughput for the TODO's benchmark targets: committing 10,000
//! entities, diffing two large trees, merging two divergent branches, and
//! finding the merge base of two long histories.
//!
//! Run with `cargo bench --bench throughput`. Timings are single runs on the
//! current machine, not statistical benchmarks.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tpt_version::diff::diff_commits;
use tpt_version::error::ObjectError;
use tpt_version::{
    ActorId, Entity, EntityId, LogicalClock, MemoryStore, RepoError, Repository, Scalar, UirNode,
};

const ENTITIES: u32 = 10_000;

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

fn report(label: &str, elapsed: Duration, items: usize) {
    println!(
        "{label:<44} {:>9.1} ms  ({items} entities)",
        elapsed.as_secs_f64() * 1e3
    );
}

fn main() -> Result<(), RepoError> {
    let author = ActorId::from_bytes([1; 16]);
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;

    let started = Instant::now();
    for id in 0..ENTITIES {
        repo.put_entity(&Cell { id, value: 0 })?;
    }
    let base = repo.commit("base", &author)?;
    report(
        "commit 10,000 entities",
        started.elapsed(),
        ENTITIES as usize,
    );

    // Two branches edit disjoint halves, so the merge needs no conflict resolution.
    repo.branch("left", base)?;
    repo.branch("right", base)?;
    repo.switch_branch("left")?;
    for id in (0..ENTITIES).step_by(2) {
        repo.put_entity(&Cell { id, value: 1 })?;
    }
    let left = repo.commit("edit evens", &author)?;
    repo.switch_branch("right")?;
    for id in (1..ENTITIES).step_by(2) {
        repo.put_entity(&Cell { id, value: 2 })?;
    }
    repo.commit("edit odds", &author)?;

    let started = Instant::now();
    let changes = diff_commits(repo.store(), &base, &left)?;
    report(
        "diff two 10,000-entity trees",
        started.elapsed(),
        changes.len(),
    );

    repo.switch_branch("left")?;
    let started = Instant::now();
    let merged = repo.merge("right")?;
    report(
        "merge two divergent branches",
        started.elapsed(),
        ENTITIES as usize,
    );
    assert!(
        merged.conflicts.is_empty(),
        "disjoint edits must not conflict"
    );
    repo.commit("merge right", &author)?;
    deep_history(&author)
}

/// Two branches with long histories of their own, forked from a 2,000-commit
/// trunk. The merge has to find the fork point through all of that history.
fn deep_history(author: &ActorId) -> Result<(), RepoError> {
    const TRUNK: u32 = 2_000;
    const SIDE: u32 = 1_000;

    let mut repo = Repository::open(
        MemoryStore::default(),
        LogicalClock::starting_at(1),
        "trunk",
    )?;
    let mut fork = None;
    for step in 0..TRUNK {
        repo.put_entity(&Cell {
            id: step % 16,
            value: i64::from(step),
        })?;
        let commit = repo.commit("trunk", author)?;
        if step == TRUNK / 2 {
            fork = Some(commit);
        }
    }
    repo.branch("side", fork.expect("fork point is set"))?;
    repo.switch_branch("side")?;
    for step in 0..SIDE {
        // Ids 100 and up, so the side branch never edits what the trunk edits.
        repo.put_entity(&Cell {
            id: 100 + step % 16,
            value: i64::from(step),
        })?;
        repo.commit("side", author)?;
    }

    let started = Instant::now();
    let merged = repo.merge("trunk")?;
    report(
        "merge 2,000- and 1,000-commit histories",
        started.elapsed(),
        (TRUNK + SIDE) as usize,
    );
    assert!(
        merged.conflicts.is_empty(),
        "disjoint edits must not conflict"
    );
    Ok(())
}
