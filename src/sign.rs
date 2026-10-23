//! Commit signing with Ed25519. A signature covers a commit's canonical bytes
//! without the signature field itself.

use tpt_crypto_sig::ed25519::{Signature as Ed25519Signature, SigningKey, VerifyingKey};

/// A signature over a commit, with the public key that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The Ed25519 public key that produced the signature.
    pub signer: [u8; 32],
    /// The 64-byte Ed25519 signature.
    pub bytes: [u8; 64],
}

/// Produces commit signatures. Injected so keys can live in hardware or an agent.
pub trait Signer {
    /// The 32-byte public key that verifies this signer's signatures.
    fn public_key(&self) -> [u8; 32];
    /// Signs `message`, returning the 64-byte signature.
    fn sign(&self, message: &[u8]) -> [u8; 64];
}

/// An Ed25519 signer built from a 32-byte secret seed.
pub struct Ed25519Signer {
    key: SigningKey,
}

impl Ed25519Signer {
    /// A signer for the key derived from `seed`.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_seed(seed),
        }
    }
}

impl Signer for Ed25519Signer {
    fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

/// Whether `signature` is a valid signature of `message` by its signer.
///
/// The strict checks, including rejection of small-order keys and commitments,
/// live in `tpt-crypto-sig`'s `VerifyingKey::verify`.
pub fn verify(signature: &Signature, message: &[u8]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(&signature.signer) else {
        return false;
    };
    key.verify(message, &Ed25519Signature::from_bytes(signature.bytes))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_message_verifies_and_other_messages_do_not() {
        let signer = Ed25519Signer::from_seed([3; 32]);
        let signature = Signature {
            signer: signer.public_key(),
            bytes: signer.sign(b"commit"),
        };
        assert!(verify(&signature, b"commit"));
        assert!(!verify(&signature, b"other"));
    }

    #[test]
    fn all_zero_key_and_signature_are_rejected() {
        let forged = Signature {
            signer: [0; 32],
            bytes: [0; 64],
        };
        assert!(!verify(&forged, b"anything"));
    }
}
