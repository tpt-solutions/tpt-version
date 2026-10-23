//! Property tests: every generated object must survive encode then decode
//! unchanged. Failures are shrunk by proptest to a minimal case.

mod common;

use std::collections::BTreeMap;

use common::vertex;
use proptest::prelude::*;
use tpt_testkit::property::check_round_trip;
use tpt_version::object::{Commit, Object, Tree};
use tpt_version::{ActorId, ContentHash, Entity, EntityId, ObjectError};

fn hash() -> impl Strategy<Value = ContentHash> {
    prop::collection::vec(any::<u8>(), 0..32).prop_map(|bytes| ContentHash::of(&bytes))
}

fn entity_id() -> impl Strategy<Value = EntityId> {
    any::<[u8; 16]>().prop_map(EntityId::from_bytes)
}

fn entity() -> impl Strategy<Value = Object> {
    // NaN never equals itself, so it cannot round-trip under PartialEq.
    let finite = prop::num::f64::NORMAL | prop::num::f64::ZERO;
    (any::<u8>(), finite, finite, finite)
        .prop_map(|(n, x, y, z)| Object::Entity(vertex(n, [x, y, z]).to_uir()))
}

fn tree() -> impl Strategy<Value = Object> {
    prop::collection::btree_map(entity_id(), hash(), 0..8)
        .prop_map(|entries: BTreeMap<_, _>| Object::Tree(Tree { entries }))
}

fn commit() -> impl Strategy<Value = Object> {
    (
        hash(),
        prop::collection::vec(hash(), 0..3),
        any::<[u8; 16]>(),
        any::<u64>(),
        "[ -~]{0,40}",
    )
        .prop_map(|(tree_hash, parents, author, timestamp, message)| {
            Object::Commit(Commit {
                tree_hash,
                parents,
                author: ActorId::from_bytes(author),
                timestamp,
                message,
                signature: None,
            })
        })
}

fn decode(bytes: &[u8]) -> Result<Object, ObjectError> {
    Object::decode(bytes)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn objects_round_trip(object in prop_oneof![entity(), tree(), commit()]) {
        prop_assert_eq!(check_round_trip(&object, Object::encode, decode), Ok(()));
    }
}
