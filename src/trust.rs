//! Trust policy: which keys may sign the commits on a branch.
//!
//! A signature proves which key made a commit. A policy says which keys the
//! application accepts. Branches named in the policy take new commits only when
//! they are signed by a trusted key, and merges into them are checked the same
//! way. Branches outside the policy are unaffected.

use alloc::collections::BTreeSet;
use alloc::string::String;

use crate::object::Commit;
use crate::sign;

/// The keys a repository trusts, and the branches that must be signed by one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustPolicy {
    trusted_keys: BTreeSet<[u8; 32]>,
    signed_branches: BTreeSet<String>,
}

impl TrustPolicy {
    /// A policy that trusts no keys and governs no branches.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accepts commits signed by the key `public_key`.
    pub fn trust_key(mut self, public_key: [u8; 32]) -> Self {
        self.trusted_keys.insert(public_key);
        self
    }

    /// Requires every new commit on `branch`, and every merge into it, to be
    /// signed by a trusted key.
    pub fn require_signed(mut self, branch: impl Into<String>) -> Self {
        self.signed_branches.insert(branch.into());
        self
    }

    /// Whether `branch` is governed by this policy.
    pub fn governs(&self, branch: &str) -> bool {
        self.signed_branches.contains(branch)
    }

    /// Whether `commit` carries a valid signature from a trusted key.
    pub(crate) fn accepts(&self, commit: &Commit) -> bool {
        commit.signature.as_ref().is_some_and(|signature| {
            self.trusted_keys.contains(&signature.signer)
                && sign::verify(signature, &commit.signing_bytes())
        })
    }
}
