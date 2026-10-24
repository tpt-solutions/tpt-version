//! A live-collaboration op log: replicas edit an entity map offline and exchange
//! their operations later, and every replica ends up with the same state.
//!
//! This sits beside the commit DAG rather than inside it. The DAG records
//! history that people review and merge on purpose. The op log records edits as
//! they happen. Each operation carries a Lamport stamp `(lamport, replica)`, and
//! for each entity the operation with the highest stamp decides its state: a
//! put leaves the value, a remove deletes it. Because the rule looks only at the
//! set of operations, merging logs in any order gives the same result.

use alloc::collections::BTreeMap;

use crate::object::{ActorId, EntityId};
use crate::uir::UirNode;

/// What an operation does to its entity.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Set the entity to this value.
    Put(UirNode),
    /// Delete the entity.
    Remove,
}

/// One edit, stamped so that every replica orders it the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    /// The replica that made the edit.
    pub replica: ActorId,
    /// The replica's Lamport clock when it made the edit.
    pub lamport: u64,
    /// The entity the edit applies to.
    pub entity: EntityId,
    /// What the edit does.
    pub change: Change,
}

/// The operations a replica has seen, keyed by their stamp.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpLog {
    ops: BTreeMap<(u64, ActorId), Op>,
}

impl OpLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of distinct operations.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether the log holds no operations.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The highest Lamport clock in the log, or 0 when it is empty.
    pub fn max_lamport(&self) -> u64 {
        self.ops
            .keys()
            .next_back()
            .map_or(0, |(lamport, _)| *lamport)
    }

    /// Adds an operation. An operation with a stamp already present is ignored,
    /// so merging the same log twice changes nothing.
    pub fn insert(&mut self, op: Op) {
        self.ops.entry((op.lamport, op.replica)).or_insert(op);
    }

    /// Adds every operation from `other`.
    pub fn merge(&mut self, other: &OpLog) {
        for op in other.ops.values() {
            self.insert(op.clone());
        }
    }

    /// The state that the log implies: each entity's value, from the operation
    /// with the highest stamp. Entities whose winning operation is a remove are
    /// absent.
    pub fn state(&self) -> BTreeMap<EntityId, UirNode> {
        let mut winners: BTreeMap<EntityId, &Op> = BTreeMap::new();
        for op in self.ops.values() {
            // Operations come in ascending stamp order, so the last one wins.
            winners.insert(op.entity, op);
        }
        winners
            .into_iter()
            .filter_map(|(entity, op)| match &op.change {
                Change::Put(value) => Some((entity, value.clone())),
                Change::Remove => None,
            })
            .collect()
    }
}

/// A participant in the collaboration: it stamps its own edits and keeps its own
/// copy of the log.
#[derive(Debug, Clone)]
pub struct Replica {
    id: ActorId,
    log: OpLog,
}

impl Replica {
    /// A replica with an empty log.
    pub fn new(id: ActorId) -> Self {
        Self {
            id,
            log: OpLog::new(),
        }
    }

    /// This replica's identity.
    pub fn id(&self) -> ActorId {
        self.id
    }

    /// The log this replica holds.
    pub fn log(&self) -> &OpLog {
        &self.log
    }

    /// The state this replica currently sees.
    pub fn state(&self) -> BTreeMap<EntityId, UirNode> {
        self.log.state()
    }

    /// Sets an entity to `value`, locally.
    pub fn put(&mut self, entity: EntityId, value: UirNode) {
        self.record(entity, Change::Put(value));
    }

    /// Removes an entity, locally.
    pub fn remove(&mut self, entity: EntityId) {
        self.record(entity, Change::Remove);
    }

    /// Takes in another replica's log. Afterwards this replica's clock is past
    /// every stamp it has seen, so its next edit wins over what it has merged.
    pub fn receive(&mut self, log: &OpLog) {
        self.log.merge(log);
    }

    fn record(&mut self, entity: EntityId, change: Change) {
        let lamport = self.log.max_lamport() + 1;
        self.log.insert(Op {
            replica: self.id,
            lamport,
            entity,
            change,
        });
    }
}
