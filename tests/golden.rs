//! Golden fixtures pin the canonical bytes of each object kind. Every platform
//! in CI must reproduce them exactly, so an encoding change shows up as a
//! fixture diff.
//!
//! Regenerate after an intended format change with
//! `TPT_UPDATE_GOLDEN=1 cargo test --test golden`, then review the diff.

mod common;

use std::collections::BTreeMap;

use common::{author, id, vertex};
use tpt_testkit::golden::assert_bytes;
use tpt_version::object::{Commit, Object, Tree};
use tpt_version::{ContentHash, Entity};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn entity_bytes() {
    let object = Object::Entity(vertex(1, [0.0, 1.5, -2.0]).to_uir());
    assert_bytes(MANIFEST, "entity_bytes.expected.json", object.encode());
}

#[test]
fn tree_bytes() {
    let entries = BTreeMap::from([
        (id(1), ContentHash::of(b"first entity")),
        (id(2), ContentHash::of(b"second entity")),
    ]);
    let object = Object::Tree(Tree { entries });
    assert_bytes(MANIFEST, "tree_bytes.expected.json", object.encode());
}

#[test]
fn commit_bytes() {
    let object = Object::Commit(Commit {
        tree_hash: ContentHash::of(b"snapshot"),
        parents: vec![ContentHash::of(b"parent")],
        author: author(),
        timestamp: 1_700_000_000,
        message: "golden commit".into(),
        signature: None,
    });
    assert_bytes(MANIFEST, "commit_bytes.expected.json", object.encode());
}
