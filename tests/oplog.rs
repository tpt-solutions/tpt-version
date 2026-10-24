//! The live-collaboration op log: replicas converge once they have exchanged
//! their operations, whatever order the exchanges happened in.

use std::collections::BTreeMap;

use proptest::prelude::*;
use tpt_version::oplog::{OpLog, Replica};
use tpt_version::{ActorId, EntityId, Scalar, UirNode};

fn id(n: u8) -> EntityId {
    EntityId::from_bytes([n; 16])
}

fn value(n: i64) -> UirNode {
    UirNode::Scalar(Scalar::Int(n))
}

fn replica(n: u8) -> Replica {
    Replica::new(ActorId::from_bytes([n; 16]))
}

#[test]
fn offline_edits_merge_and_the_later_edit_wins() {
    let mut laptop = replica(1);
    let mut desk = replica(2);
    laptop.put(id(1), value(10));
    desk.receive(laptop.log());
    desk.put(id(1), value(20));

    // Both edit id 1 while apart. Each has seen the same starting point, so the
    // desk's later stamp is the one that counts once the logs meet.
    laptop.put(id(2), value(5));
    laptop.receive(desk.log());
    desk.receive(laptop.log());
    assert_eq!(laptop.state(), desk.state());
    assert_eq!(laptop.state().get(&id(1)), Some(&value(20)));
    assert_eq!(laptop.state().get(&id(2)), Some(&value(5)));
}

#[test]
fn a_remove_beats_an_earlier_put_and_loses_to_a_later_one() {
    let mut a = replica(1);
    let mut b = replica(2);
    a.put(id(1), value(1));
    b.receive(a.log());
    b.remove(id(1));
    a.receive(b.log());
    assert!(!a.state().contains_key(&id(1)));

    a.put(id(1), value(2));
    b.receive(a.log());
    assert_eq!(b.state().get(&id(1)), Some(&value(2)));
}

#[test]
fn merging_a_log_twice_changes_nothing() {
    let mut a = replica(1);
    a.put(id(1), value(1));
    let mut once = OpLog::new();
    once.merge(a.log());
    let mut twice = once.clone();
    twice.merge(a.log());
    assert_eq!(once, twice);
    assert_eq!(twice.len(), 1);
}

#[derive(Debug, Clone)]
enum Action {
    Put { at: usize, entity: u8, value: i64 },
    Remove { at: usize, entity: u8 },
    Sync { from: usize, to: usize },
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0usize..3, 0u8..6, -50i64..50).prop_map(|(at, entity, value)| Action::Put {
            at,
            entity,
            value
        }),
        (0usize..3, 0u8..6).prop_map(|(at, entity)| Action::Remove { at, entity }),
        (0usize..3, 0usize..3).prop_map(|(from, to)| Action::Sync { from, to }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn replicas_converge_after_all_pairs_have_synced(actions in prop::collection::vec(action(), 0..80)) {
        let mut replicas: Vec<Replica> = (1..=3).map(replica).collect();
        for action in actions {
            match action {
                Action::Put { at, entity, value: v } => replicas[at].put(id(entity), value(v)),
                Action::Remove { at, entity } => replicas[at].remove(id(entity)),
                Action::Sync { from, to } => {
                    let log = replicas[from].log().clone();
                    replicas[to].receive(&log);
                }
            }
        }
        // Every replica takes in every other log, in both directions, until none
        // of them changes. Afterwards they must all agree.
        for _ in 0..2 {
            for from in 0..3 {
                for to in 0..3 {
                    let log = replicas[from].log().clone();
                    replicas[to].receive(&log);
                }
            }
        }
        let first: BTreeMap<EntityId, UirNode> = replicas[0].state();
        for other in &replicas[1..] {
            prop_assert_eq!(other.state(), first.clone());
        }
    }
}

fn own_ops(replica: &Replica) -> Vec<tpt_version::oplog::Op> {
    replica
        .log()
        .ops()
        .filter(|op| op.replica == replica.id())
        .cloned()
        .collect()
}

#[test]
fn an_operation_waits_for_the_earlier_ones_from_its_author() {
    let mut author = replica(1);
    author.put(id(1), value(1));
    author.put(id(1), value(2));
    let [first, second] = <[_; 2]>::try_from(own_ops(&author)).unwrap();

    let mut reader = replica(2);
    reader.deliver(second);
    assert_eq!(reader.pending(), 1);
    assert!(reader.state().is_empty());

    reader.deliver(first);
    assert_eq!(reader.pending(), 0);
    assert_eq!(reader.state().get(&id(1)), Some(&value(2)));
}

#[test]
fn an_operation_waits_for_what_its_author_had_seen() {
    let mut a = replica(1);
    a.put(id(1), value(1));

    // B sees A's edit, then makes one that depends on it.
    let mut b = replica(2);
    b.receive(a.log());
    b.put(id(2), value(2));
    let [from_b] = <[_; 1]>::try_from(own_ops(&b)).unwrap();

    // C gets B's edit before A's, so it has to hold B's edit back.
    let mut c = replica(3);
    c.deliver(from_b);
    assert_eq!(c.pending(), 1);
    c.receive(a.log());
    assert_eq!(c.pending(), 0);
    assert_eq!(c.state(), b.state());
}

#[test]
fn delivering_an_operation_twice_changes_nothing() {
    let mut a = replica(1);
    a.put(id(1), value(1));
    let [op] = <[_; 1]>::try_from(own_ops(&a)).unwrap();

    let mut b = replica(2);
    b.deliver(op.clone());
    b.deliver(op);
    assert_eq!(b.log().len(), 1);
    assert_eq!(b.pending(), 0);
}

#[test]
fn a_saved_log_loads_back_and_the_replica_resumes_its_count() {
    let mut a = replica(1);
    a.put(id(1), value(1));
    a.put(id(2), value(2));

    let mut store = tpt_version::MemoryStore::default();
    let hash = a.log().save(&mut store).unwrap();
    assert_eq!(
        a.log().save(&mut store).unwrap(),
        hash,
        "same log, same address"
    );
    let loaded = OpLog::load(&store, &hash).unwrap();
    assert_eq!(&loaded, a.log());

    let mut resumed = Replica::from_log(a.id(), loaded);
    resumed.put(id(3), value(3));
    let newest = own_ops(&resumed).pop().unwrap();
    assert_eq!(newest.seq, 3);

    // A reader that has everything can take the resumed replica's edits.
    let mut reader = replica(2);
    reader.receive(a.log());
    reader.receive(resumed.log());
    assert_eq!(reader.pending(), 0);
    assert_eq!(reader.state(), resumed.state());
}
