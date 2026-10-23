mod common;

use std::collections::BTreeMap;

use common::vertex;
use tpt_version::error::ObjectError;
use tpt_version::object::{Commit, Conflict, FieldConflict, Object, Tree};
use tpt_version::{ActorId, ContentHash, Entity, EntityId, Scalar, Signature, UirNode};

fn sample_entity() -> UirNode {
    UirNode::Map(BTreeMap::from([
        (
            "name".into(),
            UirNode::Scalar(Scalar::Text("bracket".into())),
        ),
        ("count".into(), UirNode::from(3_i64)),
        ("tags".into(), UirNode::List(vec![UirNode::from("steel")])),
        ("flags".into(), UirNode::Scalar(Scalar::Bool(true))),
        ("blob".into(), UirNode::Scalar(Scalar::Bytes(vec![0, 255]))),
        ("empty".into(), UirNode::Scalar(Scalar::Null)),
    ]))
}

#[test]
fn every_object_kind_round_trips() {
    let mut entries = BTreeMap::new();
    entries.insert(EntityId::from_bytes([3; 16]), ContentHash::of(b"a"));
    let objects = [
        Object::Blob(b"raw mesh".to_vec()),
        Object::Entity(sample_entity()),
        Object::Tree(Tree { entries }),
        Object::Commit(Commit {
            tree_hash: ContentHash::of(b"tree"),
            parents: vec![ContentHash::of(b"p1"), ContentHash::of(b"p2")],
            author: ActorId::from_bytes([1; 16]),
            timestamp: 42,
            message: "signed".into(),
            signature: Some(Signature {
                signer: [9; 32],
                bytes: [8; 64],
            }),
        }),
        Object::Conflict(Conflict {
            entity: EntityId::from_bytes([4; 16]),
            base: Some(ContentHash::of(b"base")),
            ours: None,
            theirs: Some(ContentHash::of(b"theirs")),
            fields: vec![FieldConflict {
                path: vec!["position".into(), "x".into()],
                base: Some(UirNode::from(0.0)),
                ours: None,
                theirs: Some(UirNode::from(1.0)),
            }],
        }),
    ];
    for object in objects {
        let bytes = object.encode();
        assert_eq!(Object::decode(&bytes).unwrap(), object, "{}", object.kind());
    }
}

#[test]
fn identical_content_has_identical_hash() {
    let first = Object::Entity(vertex(1, [1.0, 2.0, 3.0]).to_uir());
    let second = Object::Entity(vertex(1, [1.0, 2.0, 3.0]).to_uir());
    assert_eq!(first.hash(), second.hash());
    assert_ne!(
        first.hash(),
        Object::Entity(vertex(1, [1.0, 2.0, 3.5]).to_uir()).hash()
    );
}

#[test]
fn type_tag_separates_identical_payloads() {
    // A blob and an entity holding the same bytes must not collide.
    let blob = Object::Blob(vec![2, 0, 0, 0, 0, 0, 0, 0, 0]);
    let entity = Object::Entity(UirNode::Scalar(Scalar::Bytes(vec![])));
    assert_ne!(blob.hash(), entity.hash());
}

/// The FIPS 180-4 vector for "abc", so the pinned hashes below rest on a verified SHA-256.
#[test]
fn content_hash_is_sha256() {
    assert_eq!(
        ContentHash::of(b"abc").to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// Pins the encoding. A change here means existing hashes change, so it must be deliberate.
#[test]
fn entity_encoding_is_pinned() {
    let hash = Object::Entity(vertex(1, [0.0, 0.0, 0.0]).to_uir()).hash();
    assert_eq!(
        hash.to_string(),
        "fafb5d3419c7eb0cc2ac18e5ab2665329b14315135858ff052c316c3750f7f98"
    );
}

#[test]
fn truncated_input_is_rejected() {
    let mut bytes = Object::Blob(b"abcdef".to_vec()).encode();
    bytes.pop();
    assert_eq!(Object::decode(&bytes), Err(ObjectError::Truncated));
    assert_eq!(Object::decode(&[]), Err(ObjectError::Truncated));
}

#[test]
fn trailing_bytes_are_rejected() {
    let mut bytes = Object::Blob(b"abc".to_vec()).encode();
    bytes.push(0);
    assert_eq!(Object::decode(&bytes), Err(ObjectError::TrailingBytes));
}

#[test]
fn unknown_tag_is_rejected() {
    assert_eq!(Object::decode(&[99]), Err(ObjectError::UnknownTag(99)));
}

#[test]
fn hex_identifiers_parse_back() {
    let hash = ContentHash::of(b"round trip");
    let parsed: ContentHash = hash.to_string().parse().unwrap();
    assert_eq!(parsed, hash);
    assert!("xyz".parse::<ContentHash>().is_err());
}
