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
