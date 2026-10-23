//! Commit signing with Ed25519. A signature covers a commit's canonical bytes
//! without the signature field itself.

use tpt_crypto_curve::edwards25519::EdwardsPoint;
use tpt_crypto_field::CtEq;
use tpt_crypto_sig::ed25519::{Signature as Ed25519Signature, SigningKey, VerifyingKey};

/// A signature over a commit, with the public key that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub signer: [u8; 32],
    pub bytes: [u8; 64],
}

/// Produces commit signatures. Injected so keys can live in hardware or an agent.
pub trait Signer {
    fn public_key(&self) -> [u8; 32];
    fn sign(&self, message: &[u8]) -> [u8; 64];
}

/// An Ed25519 signer built from a 32-byte secret seed.
pub struct Ed25519Signer {
    key: SigningKey,
}

impl Ed25519Signer {
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
/// Rejects small-order keys and commitments as well as the upstream checks.
/// Otherwise an all-zero key and signature would verify for any message, because
/// the cofactored equation vanishes on torsion points. This matches
/// `verify_strict` in the Ed25519 ecosystem.
pub fn verify(signature: &Signature, message: &[u8]) -> bool {
    let (Some(signer), Some(commitment)) = (
        decode_point(&signature.signer),
        decode_point(signature.bytes[..32].try_into().expect("32 bytes")),
    ) else {
        return false;
    };
    if is_small_order(&signer) || is_small_order(&commitment) {
        return false;
    }
    let Ok(key) = VerifyingKey::from_bytes(&signature.signer) else {
        return false;
    };
    key.verify(message, &Ed25519Signature::from_bytes(signature.bytes))
        .is_ok()
}

fn decode_point(bytes: &[u8; 32]) -> Option<EdwardsPoint> {
    let point = EdwardsPoint::decompress(bytes);
    if point.is_some().into_bool() {
        Some(point.unwrap())
    } else {
        None
    }
}

/// Whether `point` has order dividing 8, so multiplying by the cofactor reaches the identity.
fn is_small_order(point: &EdwardsPoint) -> bool {
    point
        .mul_by_cofactor()
        .ct_eq(&EdwardsPoint::identity())
        .into_bool()
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
