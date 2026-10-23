//! Checks run when an entity is staged: it must load back under its own id and
//! within the nesting depth that decoding accepts. Floats compare by bit pattern.

mod common;

use std::collections::BTreeMap;

use common::{author, id, new_repo, vertex};
use tpt_version::diff::diff_entities;
use tpt_version::{Entity, EntityId, ObjectError, RepoError, Scalar, UirNode};

/// An entity whose stored form carries a nested list `depth` levels deep.
struct Nested {
    id: EntityId,
    depth: usize,
}

impl Entity for Nested {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        let mut body = UirNode::from(1i64);
        for _ in 0..self.depth {
            body = UirNode::List(vec![body]);
        }
        UirNode::Map(BTreeMap::from([
            (
                "id".into(),
                UirNode::Scalar(Scalar::Bytes(self.id.as_bytes().to_vec())),
            ),
            ("body".into(), body),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("nested"));
        };
        let Some(UirNode::Scalar(Scalar::Bytes(bytes))) = fields.get("id") else {
            return Err(ObjectError::InvalidValue("nested id"));
        };
        let bytes = <[u8; 16]>::try_from(bytes.as_slice())
            .map_err(|_| ObjectError::InvalidValue("nested id length"))?;
        Ok(Self {
            id: EntityId::from_bytes(bytes),
            depth: 0,
        })
    }
}

/// An entity whose stored form names a different id than `id()` returns.
struct Mislabelled {
    id: EntityId,
    stored: EntityId,
}

impl Entity for Mislabelled {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        UirNode::Map(BTreeMap::from([(
            "id".into(),
            UirNode::Scalar(Scalar::Bytes(self.stored.as_bytes().to_vec())),
        )]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("mislabelled"));
        };
        let Some(UirNode::Scalar(Scalar::Bytes(bytes))) = fields.get("id") else {
            return Err(ObjectError::InvalidValue("mislabelled id"));
        };
        let bytes = <[u8; 16]>::try_from(bytes.as_slice())
            .map_err(|_| ObjectError::InvalidValue("mislabelled id length"))?;
        let stored = EntityId::from_bytes(bytes);
        Ok(Self { id: stored, stored })
    }
}

#[test]
fn entity_nested_past_the_decoder_limit_is_refused() {
    let mut repo = new_repo();
    let too_deep = Nested {
        id: id(1),
        depth: 200,
    };
    assert_eq!(
        repo.put_entity(&too_deep),
        Err(RepoError::Object(ObjectError::InvalidValue(
            "nesting depth"
        )))
    );
    assert!(repo.entity::<Nested>(&id(1)).unwrap().is_none());
}

#[test]
fn entity_within_the_limit_is_stored_and_loads() {
    let mut repo = new_repo();
    repo.put_entity(&Nested {
        id: id(1),
        depth: 100,
    })
    .unwrap();
    repo.commit("deep", &author()).unwrap();
    assert!(repo.entity::<Nested>(&id(1)).unwrap().is_some());
}

#[test]
fn entity_that_loads_under_another_id_is_refused() {
    let mut repo = new_repo();
    let liar = Mislabelled {
        id: id(1),
        stored: id(2),
    };
    assert_eq!(repo.put_entity(&liar), Err(RepoError::IdMismatch(id(1))));
    assert!(repo.entity::<Mislabelled>(&id(1)).unwrap().is_none());
}

#[test]
fn unchanged_nan_field_is_not_a_change() {
    let before = vertex(1, [f64::NAN, 0.0, 0.0]).to_uir();
    let after = vertex(1, [f64::NAN, 0.0, 0.0]).to_uir();
    assert!(diff_entities(&before, &after).field_changes.is_empty());
    assert_ne!(
        vertex(1, [0.0, 0.0, 0.0]).to_uir(),
        vertex(1, [-0.0, 0.0, 0.0]).to_uir(),
        "0.0 and -0.0 are different content"
    );
}
