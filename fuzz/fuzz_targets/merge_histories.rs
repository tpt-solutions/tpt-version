#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_version::maintenance::fsck;
use tpt_version::{ActorId, Entity, EntityId, LogicalClock, MemoryStore, Repository, Resolution};

#[derive(Entity, Debug, Clone, PartialEq)]
struct Cell {
    #[entity(id)]
    id: EntityId,
    value: i64,
}

const AUTHOR: ActorId = ActorId::from_bytes([3; 16]);

fn branch_name(byte: u8) -> String {
    format!("b{}", byte % 4)
}

// Each input byte triple is one operation. Errors are expected for many of
// them (conflicts, dirty working copies, missing branches), so they are ignored.
// What must hold is that nothing panics, and that once every conflict is
// settled the store is consistent.
fuzz_target!(|data: &[u8]| {
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .expect("opens");
    for op in data.chunks_exact(3) {
        let (kind, a, b) = (op[0] % 5, op[1], op[2]);
        match kind {
            0 => {
                let _ = repo.put_entity(&Cell {
                    id: EntityId::from_bytes([a % 8; 16]),
                    value: i64::from(b),
                });
            }
            1 => {
                let _ = repo.commit("step", &AUTHOR);
            }
            2 => {
                let _ = repo.switch_branch(&branch_name(a));
            }
            3 => {
                if let Some(head) = repo.head() {
                    let _ = repo.branch(&branch_name(a), head);
                }
            }
            _ => {
                let _ = repo.merge(&branch_name(a));
            }
        }
    }

    while let Some(entity) = repo.conflicts().first().map(|conflict| conflict.entity) {
        repo.resolve_conflict(&entity, Resolution::Ours)
            .expect("a listed conflict resolves");
    }
    let _ = repo.commit("final", &AUTHOR);
    let report = fsck(repo.store()).expect("fsck runs");
    assert!(report.is_clean(), "{:?}", report.problems);
});
