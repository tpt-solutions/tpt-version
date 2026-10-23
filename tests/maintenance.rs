//! fsck reports damage without stopping at the first problem, and gc removes
//! only what no ref, commit or working copy can reach.

mod common;

use common::{author, id, new_repo, vertex};
use tpt_version::dag;
use tpt_version::maintenance::{Problem, fsck};
use tpt_version::object::{Commit, ContentHash, Object, Tree};
use tpt_version::sign::Signature;
use tpt_version::store::{CasStore, MemoryStore, PruneStore, RefStore};
use tpt_version::{LogicalClock, Repository};

#[test]
fn healthy_repository_is_clean() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("one", &author()).unwrap();

    let report = fsck(repo.store()).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    assert!(report.objects_checked >= 3, "commit, tree and entity");
}

#[test]
fn missing_object_is_reported_with_its_referrer() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("one", &author()).unwrap();
    let tree = dag::load_commit(repo.store(), &head).unwrap().tree_hash;

    let mut damaged = repo.store().clone();
    assert!(damaged.delete(&tree).unwrap());

    let report = fsck(&damaged).unwrap();
    assert!(report.problems.contains(&Problem::MissingObject {
        hash: tree,
        referenced_by: Some(head),
    }));
}

#[test]
fn forged_signature_is_reported() {
    let mut store = MemoryStore::default();
    let tree_data = Object::Tree(Tree::default()).encode();
    let tree = ContentHash::of(&tree_data);
    store.put(tree, &tree_data).unwrap();

    let forged = Commit {
        tree_hash: tree,
        parents: Vec::new(),
        author: author(),
        timestamp: 1,
        message: "forged".into(),
        signature: Some(Signature {
            signer: [9; 32],
            bytes: [0; 64],
        }),
    };
    let data = Object::Commit(forged).encode();
    let hash = ContentHash::of(&data);
    store.put(hash, &data).unwrap();
    store.write_ref("main", hash).unwrap();

    let report = fsck(&store).unwrap();
    assert_eq!(report.problems, vec![Problem::BadSignature(hash)]);
}

#[test]
fn gc_removes_what_only_a_deleted_branch_held() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let kept = repo.commit("kept", &author()).unwrap();

    repo.branch("scratch", kept).unwrap();
    repo.switch_branch("scratch").unwrap();
    repo.put_entity(&vertex(2, [5.0; 3])).unwrap();
    repo.commit("scratch work", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo.delete_branch("scratch").unwrap();

    // The scratch commit, its tree and the entity only it holds.
    let report = repo.gc().unwrap();
    assert_eq!(report.removed, 3);
    assert_eq!(repo.log("main").unwrap().len(), 1);
    assert!(fsck(repo.store()).unwrap().is_clean());
    assert!(repo.entity::<common::Vertex>(&id(1)).unwrap().is_some());
}

#[test]
fn gc_keeps_uncommitted_work() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("base", &author()).unwrap();

    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    repo.gc().unwrap();

    repo.put_entity(&vertex(3, [2.0; 3])).unwrap();
    let head = repo.commit("after gc", &author()).unwrap();
    assert_eq!(repo.log("main").unwrap().len(), 2);
    assert_eq!(repo.head(), Some(head));
    assert!(repo.entity::<common::Vertex>(&id(2)).unwrap().is_some());
    assert!(fsck(repo.store()).unwrap().is_clean());
}

#[test]
fn gc_on_a_fresh_repository_removes_nothing() {
    let mut repo: Repository<MemoryStore, LogicalClock> = new_repo();
    let report = repo.gc().unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.kept, 0);
}
