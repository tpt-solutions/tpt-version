//! Criss-cross histories: two branches have merged each other's older commits,
//! so their merge bases are not unique. The merge must not depend on which base
//! happens to be newest.

mod common;

use common::{author, id};
use tpt_version::dag;
use tpt_version::prelude::*;

#[derive(Entity, Debug, Clone, PartialEq)]
struct Point {
    #[entity(id)]
    id: EntityId,
    x: i64,
    y: i64,
}

fn point(x: i64, y: i64) -> Point {
    Point { id: id(1), x, y }
}

#[test]
fn criss_cross_merge_uses_every_best_base() {
    let mut repo =
        Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main").unwrap();
    repo.put_entity(&point(0, 0)).unwrap();
    let a = repo.commit("A", &author()).unwrap();
    repo.branch("side", a).unwrap();

    // B on main moves x. C on side moves y.
    repo.put_entity(&point(1, 0)).unwrap();
    let b = repo.commit("B", &author()).unwrap();
    repo.branch("b", b).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&point(0, 2)).unwrap();
    let c = repo.commit("C", &author()).unwrap();

    // Each side merges the other's older commit, so the histories cross.
    repo.switch_branch("main").unwrap();
    repo.merge("side").unwrap();
    let m1 = repo.commit("M1", &author()).unwrap();
    repo.switch_branch("side").unwrap();
    repo.merge("b").unwrap();
    let m2 = repo.commit("M2", &author()).unwrap();

    let bases = dag::merge_bases(repo.store(), &m1, &m2).unwrap();
    let mut expected = vec![b, c];
    expected.sort();
    assert_eq!(bases, expected);

    // Each side changes the point again, in different fields.
    repo.switch_branch("main").unwrap();
    repo.put_entity(&point(5, 2)).unwrap();
    repo.commit("main moves x", &author()).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&point(1, 7)).unwrap();
    repo.commit("side moves y", &author()).unwrap();

    // Against either base alone, one field would clash. Against the virtual base
    // of both, the edits are disjoint.
    repo.switch_branch("main").unwrap();
    let result = repo.merge("side").unwrap();
    assert!(result.conflicts.is_empty(), "{:?}", result.conflicts);
    repo.commit("merge side", &author()).unwrap();
    let merged: Point = repo.entity(&id(1)).unwrap().unwrap();
    assert_eq!(merged, point(5, 7));
}
