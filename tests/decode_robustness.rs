//! Decoding must never panic, whatever bytes it is given. Corrupted objects
//! must be refused or decode to something that re-encodes exactly.

mod common;

use common::{author, new_repo, vertex};
use proptest::prelude::*;
use tpt_version::object::Object;
use tpt_version::store::CasStore;

/// A small set of valid objects to corrupt: a commit, a tree and an entity.
fn sample_objects() -> Vec<Vec<u8>> {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [1.5, -2.0, 0.0])).unwrap();
    let head = repo.commit("sample", &author()).unwrap();
    let commit = repo.store().get(&head).unwrap();
    let tree_hash = match Object::decode(&commit).unwrap() {
        Object::Commit(commit) => commit.tree_hash,
        other => panic!("expected a commit, got {other:?}"),
    };
    let tree = repo.store().get(&tree_hash).unwrap();
    let entity = match Object::decode(&tree).unwrap() {
        Object::Tree(tree) => {
            let hash = *tree.entries.values().next().unwrap();
            repo.store().get(&hash).unwrap()
        }
        other => panic!("expected a tree, got {other:?}"),
    };
    vec![commit, tree, entity]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 4000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = Object::decode(&bytes);
    }

    #[test]
    fn corrupted_objects_are_refused_or_round_trip(
        which in 0usize..3,
        index in any::<prop::sample::Index>(),
        flip in 1u8..=255,
    ) {
        let samples = sample_objects();
        let mut bytes = samples[which].clone();
        let at = index.index(bytes.len());
        bytes[at] ^= flip;
        if let Ok(object) = Object::decode(&bytes) {
            prop_assert_eq!(object.encode(), bytes);
        }
    }

    #[test]
    fn truncated_objects_are_refused(which in 0usize..3, cut in any::<prop::sample::Index>()) {
        let samples = sample_objects();
        let full = &samples[which];
        let at = cut.index(full.len());
        prop_assert!(Object::decode(&full[..at]).is_err() || at == full.len());
    }
}
