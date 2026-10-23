//! Trust policy: a governed branch takes only commits signed by a trusted key,
//! and a merge into it is checked the same way.

mod common;

use common::{author, new_repo, vertex};
use tpt_version::sign::{Ed25519Signer, Signer};
use tpt_version::trust::TrustPolicy;
use tpt_version::{RepoError, Repository};

const TRUSTED_SEED: [u8; 32] = [7; 32];
const STRANGER_SEED: [u8; 32] = [8; 32];

fn policy() -> TrustPolicy {
    let trusted = Ed25519Signer::from_seed(TRUSTED_SEED).public_key();
    TrustPolicy::new().trust_key(trusted).require_signed("main")
}

fn signed_by(seed: [u8; 32]) -> Box<Ed25519Signer> {
    Box::new(Ed25519Signer::from_seed(seed))
}

#[test]
fn governed_branch_takes_only_trusted_signatures() {
    let mut unsigned = new_repo().with_trust_policy(policy());
    unsigned.put_entity(&vertex(1, [0.0; 3])).unwrap();
    assert_eq!(
        unsigned.commit("unsigned", &author()),
        Err(RepoError::Untrusted("main".into()))
    );

    let mut stranger = new_repo()
        .with_signer(signed_by(STRANGER_SEED))
        .with_trust_policy(policy());
    stranger.put_entity(&vertex(1, [0.0; 3])).unwrap();
    assert_eq!(
        stranger.commit("stranger", &author()),
        Err(RepoError::Untrusted("main".into()))
    );

    let mut trusted = new_repo()
        .with_signer(signed_by(TRUSTED_SEED))
        .with_trust_policy(policy());
    trusted.put_entity(&vertex(1, [0.0; 3])).unwrap();
    trusted.commit("trusted", &author()).unwrap();
    assert_eq!(trusted.log("main").unwrap().len(), 1);
}

#[test]
fn merge_bringing_in_an_untrusted_commit_is_refused() {
    // Build history with the stranger's key, before the policy applies.
    let mut repo = new_repo().with_signer(signed_by(STRANGER_SEED));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    repo.commit("side work", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo.put_entity(&vertex(3, [1.0; 3])).unwrap();
    repo.commit("main work", &author()).unwrap();

    let mut governed: Repository<_, _> = repo.with_trust_policy(policy());
    assert!(matches!(
        governed.merge("side"),
        Err(RepoError::Untrusted(name)) if name == "main"
    ));
}

#[test]
fn merge_of_trusted_commits_goes_through() {
    let mut repo = new_repo().with_signer(signed_by(TRUSTED_SEED));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    repo.commit("side work", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo.put_entity(&vertex(3, [1.0; 3])).unwrap();
    repo.commit("main work", &author()).unwrap();

    let mut governed = repo.with_trust_policy(policy());
    governed.merge("side").unwrap();
    governed.commit("merge side", &author()).unwrap();
    assert_eq!(governed.log("main").unwrap().len(), 4);
}
