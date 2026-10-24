//! Large snapshots are split into shards by the first byte of the entity id.
//! The split is invisible to callers, but an edit stores only what changed.

use tpt_version::dag;
use tpt_version::maintenance::fsck;
use tpt_version::object::Object;
use tpt_version::prelude::*;
use tpt_version::store::{CasStore, PruneStore};

#[derive(Entity, Debug, Clone, PartialEq)]
struct Cell {
    #[entity(id)]
    id: EntityId,
    value: i64,
}

/// Ids that cover every first byte, so the snapshot spreads across all shards.
fn cell_id(n: u32) -> EntityId {
    let mut bytes = [0u8; 16];
    bytes[..4].copy_from_slice(&n.to_be_bytes());
    EntityId::from_bytes(bytes)
}

fn author() -> ActorId {
    ActorId::from_bytes([1; 16])
}

fn fresh() -> Repository<MemoryStore, LogicalClock> {
    Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main").unwrap()
}

/// Total bytes of every object in the store.
fn stored_bytes(repo: &Repository<MemoryStore, LogicalClock>) -> usize {
    repo.store()
        .hashes()
        .unwrap()
        .iter()
        .map(|hash| repo.store().get(hash).unwrap().len())
        .sum()
}

#[test]
fn a_large_commit_is_stored_as_shards_and_reads_back_whole() {
    let mut repo = fresh();
    for n in 0..600 {
        repo.put_entity(&Cell {
            id: cell_id(n),
            value: i64::from(n),
        })
        .unwrap();
    }
    let head = repo.commit("bulk", &author()).unwrap();

    let tree_hash = dag::load_commit(repo.store(), &head).unwrap().tree_hash;
    assert!(matches!(
        Object::decode(&repo.store().get(&tree_hash).unwrap()).unwrap(),
        Object::TreeRoot(_)
    ));
    assert_eq!(repo.head_tree().unwrap().entries.len(), 600);

    let fetched: Cell = repo.entity(&cell_id(437)).unwrap().unwrap();
    assert_eq!(fetched.value, 437);
    assert!(fsck(repo.store()).unwrap().is_clean());
}

#[test]
fn an_edit_stores_far_less_than_the_whole_snapshot() {
    let mut repo = fresh();
    for n in 0..600 {
        repo.put_entity(&Cell {
            id: cell_id(n),
            value: 0,
        })
        .unwrap();
    }
    repo.commit("bulk", &author()).unwrap();

    let flat_size = Object::Tree(repo.head_tree().unwrap()).encode().len();
    let before = stored_bytes(&repo);
    repo.put_entity(&Cell {
        id: cell_id(7),
        value: 1,
    })
    .unwrap();
    repo.commit("touch one", &author()).unwrap();
    let growth = stored_bytes(&repo) - before;

    assert!(
        growth * 2 < flat_size,
        "an edit stored {growth} bytes, a flat snapshot is {flat_size}"
    );
}

#[test]
fn edits_in_different_shards_merge_cleanly() {
    let mut repo = fresh();
    for n in 0..600 {
        repo.put_entity(&Cell {
            id: cell_id(n),
            value: 0,
        })
        .unwrap();
    }
    let base = repo.commit("bulk", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.put_entity(&Cell {
        id: cell_id(10),
        value: 1,
    })
    .unwrap();
    repo.commit("main edit", &author()).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&Cell {
        id: cell_id(500),
        value: 2,
    })
    .unwrap();
    repo.commit("side edit", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    let result = repo.merge("side").unwrap();
    assert!(result.conflicts.is_empty());
    repo.commit("merge", &author()).unwrap();
    assert_eq!(repo.entity::<Cell>(&cell_id(10)).unwrap().unwrap().value, 1);
    assert_eq!(
        repo.entity::<Cell>(&cell_id(500)).unwrap().unwrap().value,
        2
    );
    assert!(fsck(repo.store()).unwrap().is_clean());
}

#[test]
fn a_small_snapshot_stays_flat() {
    let mut repo = fresh();
    for n in 0..10 {
        repo.put_entity(&Cell {
            id: cell_id(n),
            value: 0,
        })
        .unwrap();
    }
    let head = repo.commit("small", &author()).unwrap();
    let tree_hash = dag::load_commit(repo.store(), &head).unwrap().tree_hash;
    assert!(matches!(
        Object::decode(&repo.store().get(&tree_hash).unwrap()).unwrap(),
        Object::Tree(_)
    ));
}
