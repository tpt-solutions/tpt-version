//! Decoding must never panic, whatever bytes it is given. Corrupted objects
//! must be refused or decode to something that re-encodes exactly.

mod common;

use std::sync::OnceLock;

use common::{author, new_repo, vertex};
use proptest::prelude::*;
use tpt_version::dag;
use tpt_version::object::Object;
use tpt_version::prelude::*;
use tpt_version::store::CasStore;

#[derive(Entity, Debug, Clone, PartialEq)]
struct Cell {
    #[entity(id)]
    id: EntityId,
    value: i64,
}

/// Valid objects to corrupt: a commit, a tree, an entity and a sharded root.
fn samples() -> &'static [Vec<u8>] {
    static SAMPLES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    SAMPLES.get_or_init(|| {
        let mut repo = new_repo();
        repo.put_entity(&vertex(1, [1.5, -2.0, 0.0])).unwrap();
        let head = repo.commit("sample", &author()).unwrap();
        let commit = repo.store().get(&head).unwrap();
        let tree_hash = dag::load_commit(repo.store(), &head).unwrap().tree_hash;
        let tree = repo.store().get(&tree_hash).unwrap();
        let entity = match Object::decode(&tree).unwrap() {
            Object::Tree(tree) => {
                let hash = *tree.entries.values().next().unwrap();
                repo.store().get(&hash).unwrap()
            }
            other => panic!("expected a tree, got {other:?}"),
        };

        // Past the shard threshold, so the snapshot's root is a sample too.
        let mut big = new_repo();
        for n in 0..300u32 {
            let mut bytes = [0u8; 16];
            bytes[..4].copy_from_slice(&n.to_be_bytes());
            big.put_entity(&Cell {
                id: EntityId::from_bytes(bytes),
                value: i64::from(n),
            })
            .unwrap();
        }
        let big_head = big.commit("big", &author()).unwrap();
        let root_hash = dag::load_commit(big.store(), &big_head).unwrap().tree_hash;
        let root = big.store().get(&root_hash).unwrap();
        assert!(matches!(
            Object::decode(&root).unwrap(),
            Object::TreeRoot(_)
        ));

        vec![commit, tree, entity, root]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 4000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = Object::decode(&bytes);
    }

    #[test]
    fn corrupted_objects_are_refused_or_round_trip(
        which in 0usize..4,
        index in any::<prop::sample::Index>(),
        flip in 1u8..=255,
    ) {
        let mut bytes = samples()[which].clone();
        let at = index.index(bytes.len());
        bytes[at] ^= flip;
        if let Ok(object) = Object::decode(&bytes) {
            prop_assert_eq!(object.encode(), bytes);
        }
    }

    #[test]
    fn truncated_objects_are_refused(which in 0usize..4, cut in any::<prop::sample::Index>()) {
        let full = &samples()[which];
        let at = cut.index(full.len());
        prop_assert!(Object::decode(&full[..at]).is_err() || at == full.len());
    }
}
