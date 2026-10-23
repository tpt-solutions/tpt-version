//! Signed reviews of commits, kept under reviews/ refs.

mod common;

use common::{author, new_repo, vertex};
use tpt_version::maintenance::fsck;
use tpt_version::prelude::*;
use tpt_version::sign::Ed25519Signer;
use tpt_version::{ContentHash, Review, Verdict};

fn signed_repo(seed: u8) -> Repository<MemoryStore, LogicalClock> {
    Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")
        .unwrap()
        .with_signer(Box::new(Ed25519Signer::from_seed([seed; 32])))
}

#[test]
fn signed_review_verifies_and_a_later_review_replaces_it() {
    let mut repo = signed_repo(7);
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("change", &author()).unwrap();

    repo.review(commit, Verdict::Approve, "looks right", &author())
        .unwrap();
    let reviews = repo.reviews_of(&commit).unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].verdict, Verdict::Approve);
    assert!(reviews[0].is_valid());

    repo.review(commit, Verdict::RequestChanges, "one more thing", &author())
        .unwrap();
    let reviews = repo.reviews_of(&commit).unwrap();
    assert_eq!(reviews.len(), 1, "the same reviewer replaces their review");
    assert_eq!(reviews[0].verdict, Verdict::RequestChanges);
}

#[test]
fn unsigned_review_is_listed_but_not_valid() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("change", &author()).unwrap();
    repo.review(commit, Verdict::Comment, "no key here", &author())
        .unwrap();
    let reviews = repo.reviews_of(&commit).unwrap();
    assert_eq!(reviews.len(), 1);
    assert!(!reviews[0].is_valid());
}

#[test]
fn each_reviewer_keeps_their_own_review() {
    let mut first = signed_repo(7);
    first.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = first.commit("change", &author()).unwrap();
    first
        .review(commit, Verdict::Approve, "yes", &author())
        .unwrap();

    // A second reviewer works on the same store with their own key.
    let mut second = Repository::open(
        first.store().clone(),
        LogicalClock::starting_at(100),
        "main",
    )
    .unwrap()
    .with_signer(Box::new(Ed25519Signer::from_seed([8; 32])));
    second
        .review(commit, Verdict::RequestChanges, "no", &author())
        .unwrap();

    let reviews = second.reviews_of(&commit).unwrap();
    assert_eq!(reviews.len(), 2);
    assert!(reviews.iter().all(Review::is_valid));
    let verdicts: Vec<Verdict> = reviews.iter().map(|review| review.verdict).collect();
    assert!(verdicts.contains(&Verdict::Approve));
    assert!(verdicts.contains(&Verdict::RequestChanges));
}

#[test]
fn an_altered_review_fails_verification() {
    let mut repo = signed_repo(7);
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let commit = repo.commit("change", &author()).unwrap();
    repo.review(commit, Verdict::Approve, "fine", &author())
        .unwrap();
    let mut review = repo.reviews_of(&commit).unwrap().remove(0);
    assert!(review.is_valid());
    review.note = "approved, but actually no".into();
    assert!(!review.is_valid());
}

#[test]
fn reviewing_an_unknown_commit_is_refused() {
    let mut repo = new_repo();
    assert!(
        repo.review(ContentHash::of(b"nothing"), Verdict::Approve, "", &author())
            .is_err()
    );
}

#[test]
fn a_reviewed_commit_survives_gc_after_its_branch_is_gone() {
    let mut repo = signed_repo(7);
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let kept = repo.commit("kept", &author()).unwrap();
    repo.branch("temp", kept).unwrap();
    repo.switch_branch("temp").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    let reviewed = repo.commit("reviewed", &author()).unwrap();
    repo.review(reviewed, Verdict::Approve, "keep this", &author())
        .unwrap();
    repo.switch_branch("main").unwrap();
    repo.delete_branch("temp").unwrap();

    let report = repo.gc().unwrap();
    assert_eq!(report.removed, 0);
    assert!(fsck(repo.store()).unwrap().is_clean());
    assert_eq!(repo.list_branches().unwrap(), ["main"]);
}

#[test]
fn branch_names_cannot_use_the_review_namespace() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let head = repo.commit("base", &author()).unwrap();
    assert!(matches!(
        repo.branch("reviews/x", head),
        Err(tpt_version::RepoError::InvalidBranchName(_))
    ));
}
