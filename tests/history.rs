mod common;

use common::{Vertex, author, id, new_repo, put, vertex};
use tpt_version::dag;
use tpt_version::object::Object;
use tpt_version::sign::Signer;
use tpt_version::store::RefStore;
use tpt_version::{
    Commit, ContentHash, Ed25519Signer, LogicalClock, MemoryStore, RepoError, Repository,
    Signature, StoreError,
};

#[test]
fn linear_history_is_logged_newest_first() {
    let mut repo = new_repo();
    for message in ["c1", "c2", "c3"] {
        repo.put_entity(&vertex(1, [0.0, 0.0, 0.0])).unwrap();
        repo.commit(message, &author()).unwrap();
    }
    let messages: Vec<String> = repo
        .log("main")
        .unwrap()
        .into_iter()
        .map(|c| c.message)
        .collect();
    assert_eq!(messages, ["c3", "c2", "c1"]);
}

#[test]
fn branches_diverge_and_list() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let fork = repo.commit("fork point", &author()).unwrap();
    repo.branch("dev", fork).unwrap();
    assert_eq!(repo.list_branches().unwrap(), ["dev", "main"]);

    repo.switch_branch("dev").unwrap();
    repo.put_entity(&vertex(1, [5.0; 3])).unwrap();
    let dev_head = repo.commit("dev work", &author()).unwrap();

    assert_eq!(repo.resolve_branch("dev").unwrap(), Some(dev_head));
    assert_eq!(repo.resolve_branch("main").unwrap(), Some(fork));
    assert_eq!(repo.current_branch(), Some("dev"));
}

#[test]
fn branch_names_are_validated() {
    let mut repo = new_repo();
    let head = {
        repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
        repo.commit("init", &author()).unwrap()
    };
    for name in [
        "",
        "/lead",
        "-dash",
        "a..b",
        "a//b",
        "trail.",
        "has space",
        "x\\y",
        "feature/x.lock",
        "CON",
        "nul.txt",
        "lpt1/topic",
        "a/Aux.draft",
    ] {
        assert!(
            matches!(
                repo.branch(name, head),
                Err(RepoError::InvalidBranchName(_))
            ),
            "{name:?} should be rejected"
        );
    }
    repo.branch("feature/cad-v2", head).unwrap();
}

#[test]
fn branch_names_that_clash_are_refused() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("init", &author()).unwrap();
    repo.branch("feature/cad", head).unwrap();

    // A case-only difference is the same branch on Windows and macOS.
    assert!(matches!(
        repo.branch("Main", head),
        Err(RepoError::BranchNameClash { .. })
    ));
    assert!(matches!(
        repo.branch("FEATURE/CAD", head),
        Err(RepoError::BranchNameClash { .. })
    ));
    // `feature` cannot be a branch while `feature/cad` exists, and vice versa.
    assert!(matches!(
        repo.branch("feature", head),
        Err(RepoError::BranchNameClash { .. })
    ));
    repo.branch("feature-two", head).unwrap();
    repo.branch("feature/cad-v2", head).unwrap();
    assert_eq!(
        repo.list_branches().unwrap(),
        ["feature-two", "feature/cad", "feature/cad-v2", "main"]
    );
}

#[test]
fn duplicate_and_missing_branches_are_errors() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("init", &author()).unwrap();
    assert!(matches!(
        repo.branch("main", head),
        Err(RepoError::BranchExists(_))
    ));
    assert!(matches!(
        repo.delete_branch("nope"),
        Err(RepoError::BranchNotFound(_))
    ));
    assert!(matches!(
        repo.switch_branch("nope"),
        Err(RepoError::BranchNotFound(_))
    ));
}

#[test]
fn checked_out_branch_cannot_be_deleted() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("init", &author()).unwrap();
    repo.branch("spare", head).unwrap();
    assert!(matches!(
        repo.delete_branch("main"),
        Err(RepoError::CannotDeleteCurrentBranch(_))
    ));
    repo.delete_branch("spare").unwrap();
    assert_eq!(repo.list_branches().unwrap(), ["main"]);
}

#[test]
fn merge_base_is_the_fork_point() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let fork = repo.commit("fork", &author()).unwrap();
    repo.branch("side", fork).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    let main_head = repo.commit("main edit", &author()).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(1, [2.0; 3])).unwrap();
    let side_head = repo.commit("side edit", &author()).unwrap();

    let base = dag::find_merge_base(repo.store(), &main_head, &side_head).unwrap();
    assert_eq!(base, Some(fork));
}

#[test]
fn descendants_follow_children() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let first = repo.commit("first", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    let second = repo.commit("second", &author()).unwrap();
    repo.put_entity(&vertex(1, [2.0; 3])).unwrap();
    let third = repo.commit("third", &author()).unwrap();

    let found = dag::descendants(repo.store(), &first, &[third]).unwrap();
    assert_eq!(found.len(), 2);
    assert!(found.contains(&second) && found.contains(&third));
}

#[test]
fn checkout_restores_an_earlier_commit_detached() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let old = repo.commit("old", &author()).unwrap();
    repo.put_entity(&vertex(1, [9.0; 3])).unwrap();
    let main_head = repo.commit("new", &author()).unwrap();

    repo.checkout(old).unwrap();
    assert_eq!(repo.current_branch(), None);
    assert_eq!(
        repo.entity::<Vertex>(&id(1)).unwrap().unwrap().position,
        [0.0; 3]
    );

    // Commits made while detached leave the branch alone.
    repo.put_entity(&vertex(1, [4.0; 3])).unwrap();
    repo.commit("detached", &author()).unwrap();
    assert_eq!(repo.resolve_branch("main").unwrap(), Some(main_head));
}

#[test]
fn checkout_refuses_to_discard_uncommitted_work() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let first = repo.commit("first", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    assert_eq!(repo.checkout(first), Err(RepoError::DirtyWorkingCopy));
}

#[test]
fn reopening_resumes_the_branch_head() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [3.0; 3])).unwrap();
    let head = repo.commit("saved", &author()).unwrap();
    let store = repo.store().clone();

    let reopened = Repository::open(store, LogicalClock::starting_at(100), "main").unwrap();
    assert_eq!(reopened.head(), Some(head));
    assert_eq!(
        reopened.entity::<Vertex>(&id(1)).unwrap().unwrap().position,
        [3.0; 3]
    );
}

#[test]
fn signed_commits_verify_and_tampering_is_detected() {
    let signer = Ed25519Signer::from_seed([5; 32]);
    let public_key = signer.public_key();
    let mut repo = new_repo().with_signer(Box::new(signer));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("signed", &author()).unwrap();

    let commit = dag::load_commit(repo.store(), &head).unwrap();
    assert_eq!(commit.signature.as_ref().unwrap().signer, public_key);

    // Same content with a different message fails verification on load.
    let mut store = MemoryStore::default();
    let forged = Commit {
        message: "tampered".into(),
        ..commit.clone()
    };
    let forged_hash = put(&mut store, &Object::Commit(forged));
    assert!(matches!(
        dag::load_commit(&store, &forged_hash),
        Err(RepoError::InvalidSignature(_))
    ));
}

#[test]
fn unsigned_commits_carry_no_signature() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("plain", &author()).unwrap();
    let commit = dag::load_commit(repo.store(), &head).unwrap();
    assert!(commit.signature.is_none());
}

#[test]
fn a_bad_signature_value_is_rejected() {
    let mut store = MemoryStore::default();
    let tree = put(&mut store, &Object::Tree(Default::default()));
    let commit = Commit {
        tree_hash: tree,
        parents: vec![],
        author: author(),
        timestamp: 1,
        message: "bogus".into(),
        signature: Some(Signature {
            signer: [0; 32],
            bytes: [0; 64],
        }),
    };
    let hash = put(&mut store, &Object::Commit(commit));
    assert!(matches!(
        dag::load_commit(&store, &hash),
        Err(RepoError::InvalidSignature(_))
    ));
}

#[test]
fn missing_commit_is_a_store_error() {
    let store = MemoryStore::default();
    let missing = ContentHash::of(b"nothing");
    assert_eq!(
        dag::load_commit(&store, &missing),
        Err(RepoError::Store(StoreError::NotFound(missing)))
    );
    assert!(store.read_ref("main").unwrap().is_none());
}

/// Two branches each merge the other's older tip, so both `x1` and `y1` are
/// best common ancestors. The merge base must be chosen deterministically.
#[test]
fn criss_cross_merge_base_is_deterministic() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let root = repo.commit("root", &author()).unwrap();
    repo.branch("x", root).unwrap();
    repo.branch("y", root).unwrap();

    repo.switch_branch("x").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    let x1 = repo.commit("x1", &author()).unwrap();
    repo.branch("x1-tip", x1).unwrap();

    repo.switch_branch("y").unwrap();
    repo.put_entity(&vertex(3, [2.0; 3])).unwrap();
    let y1 = repo.commit("y1", &author()).unwrap();

    // Crosswise merges: x takes y1, then y takes x1.
    repo.switch_branch("x").unwrap();
    repo.merge("y").unwrap();
    let mx = repo.commit("mx", &author()).unwrap();
    repo.switch_branch("y").unwrap();
    repo.merge("x1-tip").unwrap();
    let my = repo.commit("my", &author()).unwrap();

    let base = dag::find_merge_base(repo.store(), &mx, &my)
        .unwrap()
        .unwrap();
    assert!(base == x1 || base == y1, "base must be one of the two tips");

    // The winner is the newer tip, ties broken by hash, so every peer agrees.
    let ts = |h| dag::load_commit(repo.store(), &h).unwrap().timestamp;
    let expected = if (ts(x1), x1) > (ts(y1), y1) { x1 } else { y1 };
    assert_eq!(base, expected);
    assert_eq!(
        dag::find_merge_base(repo.store(), &my, &mx).unwrap(),
        Some(base)
    );
}
