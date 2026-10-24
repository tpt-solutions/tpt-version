//! A live-collaboration op log: replicas edit an entity map offline and exchange
//! their operations later, and every replica ends up with the same state.
//!
//! This sits beside the commit DAG rather than inside it. The DAG records
//! history that people review and merge on purpose. The op log records edits as
//! they happen. Each operation carries a Lamport stamp `(lamport, replica)`, and
//! for each entity the operation with the highest stamp decides its state: a
//! put leaves the value, a remove deletes it. Because the rule looks only at the
//! set of operations, merging logs in any order gives the same result.
//!
//! Operations also carry causal metadata. Each replica numbers its own
//! operations, and each operation lists what its author had applied from every
//! other replica. A [`Replica`] applies an operation only after everything it
//! depends on, and holds the rest until it arrives. Logs can be saved to a
//! [`CasStore`] and loaded again. Transport is left to the caller.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::codec::{Reader, Writer};
use crate::error::{ObjectError, RepoError};
use crate::object::{ActorId, ContentHash, EntityId, Object, wrong_kind};
use crate::store::{CasStore, load_object, store_object};
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
    /// How many operations the replica had made, this one included. Counting
    /// starts at 1, and a replica's operations apply in this order.
    pub seq: u64,
    /// The operations the author had applied from each other replica, by count.
    /// The author's own operations are not listed, since `seq` covers them.
    pub seen: BTreeMap<ActorId, u64>,
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

    /// The operations in stamp order.
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.ops.values()
    }

    /// Whether the log holds the operation with this stamp.
    pub fn has_stamp(&self, lamport: u64, replica: ActorId) -> bool {
        self.ops.contains_key(&(lamport, replica))
    }

    /// For each replica, the highest `seq` in the log. A log built by a
    /// [`Replica`] holds every operation up to this count, with no gaps.
    pub fn frontier(&self) -> BTreeMap<ActorId, u64> {
        let mut frontier: BTreeMap<ActorId, u64> = BTreeMap::new();
        for op in self.ops.values() {
            let applied = frontier.entry(op.replica).or_insert(0);
            *applied = (*applied).max(op.seq);
        }
        frontier
    }

    /// Adds an operation. An operation with a stamp already present is ignored,
    /// so merging the same log twice changes nothing. This does not check causal
    /// order; use [`Replica::deliver`] for that.
    pub fn insert(&mut self, op: Op) {
        self.ops.entry((op.lamport, op.replica)).or_insert(op);
    }

    /// Adds every operation from `other`, without checking causal order.
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

    /// Stores the log as one object and returns its address. The same operations
    /// always give the same address.
    pub fn save<S: CasStore>(&self, store: &mut S) -> Result<ContentHash, RepoError> {
        store_object(store, &Object::OpLog(self.clone()))
    }

    /// Loads a log saved with [`OpLog::save`].
    pub fn load<S: CasStore>(store: &S, hash: &ContentHash) -> Result<Self, RepoError> {
        match load_object(store, hash)? {
            Object::OpLog(log) => Ok(log),
            other => Err(wrong_kind("op log", &other)),
        }
    }

    /// Writes the operations in stamp order.
    pub(crate) fn encode_into(&self, w: &mut Writer) {
        w.count(self.ops.len());
        for op in self.ops.values() {
            w.fixed(op.replica.as_bytes());
            w.u64(op.lamport);
            w.u64(op.seq);
            w.fixed(op.entity.as_bytes());
            w.count(op.seen.len());
            for (replica, seq) in &op.seen {
                w.fixed(replica.as_bytes());
                w.u64(*seq);
            }
            match &op.change {
                Change::Put(node) => {
                    w.u8(0);
                    node.encode(w);
                }
                Change::Remove => w.u8(1),
            }
        }
    }

    /// Reads a log written by [`OpLog::encode_into`]. Operations must come in
    /// strictly ascending stamp order, and `seen` in ascending replica order, so
    /// that each log has exactly one encoding.
    pub(crate) fn decode_from(r: &mut Reader<'_>) -> Result<Self, ObjectError> {
        let count = r.count()?;
        let mut log = Self::new();
        let mut previous_stamp: Option<(u64, ActorId)> = None;
        for _ in 0..count {
            let replica = ActorId::from_bytes(r.fixed()?);
            let lamport = r.u64()?;
            let seq = r.u64()?;
            let entity = EntityId::from_bytes(r.fixed()?);

            let seen_count = r.count()?;
            let mut seen = BTreeMap::new();
            let mut previous_seen: Option<ActorId> = None;
            for _ in 0..seen_count {
                let other = ActorId::from_bytes(r.fixed()?);
                let applied = r.u64()?;
                if previous_seen.is_some_and(|previous| other <= previous) {
                    return Err(ObjectError::InvalidValue("op seen order"));
                }
                previous_seen = Some(other);
                seen.insert(other, applied);
            }

            let change = match r.u8()? {
                0 => Change::Put(UirNode::decode(r)?),
                1 => Change::Remove,
                _ => return Err(ObjectError::InvalidValue("op change")),
            };

            let stamp = (lamport, replica);
            if previous_stamp.is_some_and(|previous| stamp <= previous) {
                return Err(ObjectError::InvalidValue("op order"));
            }
            previous_stamp = Some(stamp);
            log.ops.insert(
                stamp,
                Op {
                    replica,
                    lamport,
                    seq,
                    seen,
                    entity,
                    change,
                },
            );
        }
        Ok(log)
    }
}

/// A participant in the collaboration: it stamps its own edits, applies other
/// replicas' edits in causal order, and keeps its own copy of the log.
#[derive(Debug, Clone)]
pub struct Replica {
    id: ActorId,
    log: OpLog,
    pending: Vec<Op>,
}

impl Replica {
    /// A replica with an empty log.
    pub fn new(id: ActorId) -> Self {
        Self::from_log(id, OpLog::new())
    }

    /// A replica that starts from a saved log. Operations that were still
    /// waiting for their dependencies are not part of a saved log, so the
    /// caller must send them again.
    pub fn from_log(id: ActorId, log: OpLog) -> Self {
        Self {
            id,
            log,
            pending: Vec::new(),
        }
    }

    /// This replica's identity.
    pub fn id(&self) -> ActorId {
        self.id
    }

    /// The operations this replica has applied.
    pub fn log(&self) -> &OpLog {
        &self.log
    }

    /// The number of operations received but not yet applied, because something
    /// they depend on has not arrived.
    pub fn pending(&self) -> usize {
        self.pending.len()
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

    /// Takes in every operation of another replica's log, in causal order.
    /// Afterwards this replica's clock is past every stamp it has seen, so its
    /// next edit wins over what it has merged.
    pub fn receive(&mut self, log: &OpLog) {
        for op in log.ops() {
            self.deliver(op.clone());
        }
    }

    /// Takes in one operation. It is applied at once if everything it depends on
    /// has been applied, and otherwise held until those operations arrive. An
    /// operation already applied or already held is ignored.
    pub fn deliver(&mut self, op: Op) {
        let known = self.log.has_stamp(op.lamport, op.replica)
            || self
                .pending
                .iter()
                .any(|held| held.lamport == op.lamport && held.replica == op.replica);
        if known {
            return;
        }
        self.pending.push(op);
        self.apply_ready();
    }

    fn apply_ready(&mut self) {
        loop {
            let frontier = self.log.frontier();
            let Some(index) = self.pending.iter().position(|op| is_ready(op, &frontier)) else {
                break;
            };
            let op = self.pending.swap_remove(index);
            self.log.insert(op);
        }
    }

    fn record(&mut self, entity: EntityId, change: Change) {
        let mut seen = self.log.frontier();
        let seq = seen.remove(&self.id).unwrap_or(0) + 1;
        let lamport = self.log.max_lamport() + 1;
        self.log.insert(Op {
            replica: self.id,
            lamport,
            seq,
            seen,
            entity,
            change,
        });
    }
}

/// Whether everything `op` depends on is in `frontier`: the replica's earlier
/// operations, and each operation listed in `seen`.
fn is_ready(op: &Op, frontier: &BTreeMap<ActorId, u64>) -> bool {
    let applied = |replica: &ActorId| frontier.get(replica).copied().unwrap_or(0);
    applied(&op.replica) + 1 == op.seq
        && op
            .seen
            .iter()
            .all(|(replica, seq)| applied(replica) >= *seq)
}
