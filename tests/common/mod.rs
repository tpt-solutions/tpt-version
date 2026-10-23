//! Shared fixtures for the integration tests.
#![allow(dead_code)]

use std::collections::BTreeMap;

use tpt_version::error::ObjectError;
use tpt_version::object::Object;
use tpt_version::store::CasStore;
use tpt_version::{
    ActorId, ContentHash, Entity, EntityId, LogicalClock, MemoryStore, Repository, Scalar, UirNode,
};

/// A CAD vertex with a stable id and a position.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub id: EntityId,
    pub position: [f64; 3],
}

impl Entity for Vertex {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        let position = ["x", "y", "z"]
            .into_iter()
            .zip(self.position)
            .map(|(axis, value)| (axis.into(), UirNode::from(value)))
            .collect();
        UirNode::Map(BTreeMap::from([
            (
                "id".into(),
                UirNode::Scalar(Scalar::Bytes(self.id.as_bytes().to_vec())),
            ),
            ("position".into(), UirNode::Map(position)),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("vertex"));
        };
        let Some(UirNode::Scalar(Scalar::Bytes(id))) = fields.get("id") else {
            return Err(ObjectError::InvalidValue("vertex id"));
        };
        let Some(UirNode::Map(position)) = fields.get("position") else {
            return Err(ObjectError::InvalidValue("vertex position"));
        };
        let mut coords = [0.0; 3];
        for (slot, axis) in coords.iter_mut().zip(["x", "y", "z"]) {
            let Some(UirNode::Scalar(Scalar::Float(value))) = position.get(axis) else {
                return Err(ObjectError::InvalidValue("coordinate"));
            };
            *slot = *value;
        }
        let id = <[u8; 16]>::try_from(id.as_slice())
            .map_err(|_| ObjectError::InvalidValue("vertex id length"))?;
        Ok(Self {
            id: EntityId::from_bytes(id),
            position: coords,
        })
    }
}

/// A vertex whose id is `[n; 16]`.
pub fn vertex(n: u8, position: [f64; 3]) -> Vertex {
    Vertex {
        id: id(n),
        position,
    }
}

pub fn id(n: u8) -> EntityId {
    EntityId::from_bytes([n; 16])
}

pub fn author() -> ActorId {
    ActorId::from_bytes([0xA; 16])
}

pub fn new_repo() -> Repository<MemoryStore, LogicalClock> {
    Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main").unwrap()
}

/// A small xorshift generator, so runs are reproducible without a dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// Stores an object directly and returns its hash.
pub fn put(store: &mut MemoryStore, object: &Object) -> ContentHash {
    let data = object.encode();
    let hash = ContentHash::of(&data);
    store.put(hash, &data).unwrap();
    hash
}
