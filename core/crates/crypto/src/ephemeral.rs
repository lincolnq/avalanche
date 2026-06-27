//! Ephemeral Curve25519 key pairs for one-shot ECDH key agreement.
//!
//! Used by the device-linking provisioning channel (`docs/04-multi-device.md`
//! §4): each side generates a throwaway key pair, exchanges public halves
//! out-of-band (a scanned QR / pasted code) and over the relay mailbox, then
//! derives a shared secret via X25519. The raw agreement is **not** an
//! encryption key — callers must run it through a KDF (the provisioning layer
//! in `app-core` uses HKDF-SHA256).
//!
//! This is a thin wrapper over libsignal's Curve25519 so libsignal types stay
//! out of the public API — the same discipline as [`crate::identity`] and
//! [`crate::prekeys`]. The key material is held as serialized bytes (not a
//! libsignal `KeyPair`) so the type is trivially `Send + Sync` and can live in
//! an FFI object across threads.

use libsignal_protocol as signal;

// See session.rs for the `TryRngCore::unwrap_err()` pattern.
use rand::TryRngCore as _;

use crate::error::CryptoError;

/// A throwaway Curve25519 key pair for a single ECDH agreement, stored as
/// serialized public/private bytes.
#[derive(Clone)]
pub struct EphemeralKeyPair {
    private: Vec<u8>,
    public: Vec<u8>,
}

impl EphemeralKeyPair {
    /// Generate a fresh random ephemeral key pair.
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng.unwrap_err();
        let kp = signal::KeyPair::generate(&mut rng);
        Self {
            private: kp.private_key.serialize().to_vec(),
            public: kp.public_key.serialize().to_vec(),
        }
    }

    /// Reconstruct from a previously serialized private key (e.g. one held in an
    /// FFI handle across calls). The public half is recomputed.
    pub fn from_private_bytes(private: &[u8]) -> Result<Self, CryptoError> {
        let priv_key = signal::PrivateKey::deserialize(private).map_err(|_| CryptoError::InvalidKey)?;
        let public = priv_key
            .public_key()
            .map_err(|_| CryptoError::InvalidKey)?
            .serialize()
            .to_vec();
        Ok(Self { private: private.to_vec(), public })
    }

    /// The serialized public half (33 bytes: DJB type byte + 32-byte point) to
    /// embed in the pairing string and post to the mailbox.
    pub fn public_bytes(&self) -> Vec<u8> {
        self.public.clone()
    }

    /// The serialized private half — only for persisting in an FFI handle so a
    /// later call can [`EphemeralKeyPair::from_private_bytes`] it. Never leaves
    /// the device.
    pub fn private_bytes(&self) -> Vec<u8> {
        self.private.clone()
    }

    /// Compute the raw 32-byte X25519 shared secret with a peer's serialized
    /// public key. Run the result through a KDF before using it as a key.
    pub fn agree(&self, their_public_bytes: &[u8]) -> Result<[u8; 32], CryptoError> {
        let priv_key =
            signal::PrivateKey::deserialize(&self.private).map_err(|_| CryptoError::InvalidKey)?;
        let their_pub = signal::PublicKey::deserialize(their_public_bytes)
            .map_err(|_| CryptoError::InvalidKey)?;
        let shared = priv_key
            .calculate_agreement(&their_pub)
            .map_err(|_| CryptoError::InvalidKey)?;
        <[u8; 32]>::try_from(shared.as_ref()).map_err(|_| CryptoError::InvalidKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreement_is_symmetric() {
        let a = EphemeralKeyPair::generate();
        let b = EphemeralKeyPair::generate();
        let ab = a.agree(&b.public_bytes()).unwrap();
        let ba = b.agree(&a.public_bytes()).unwrap();
        assert_eq!(ab, ba, "both sides derive the same shared secret");
    }

    #[test]
    fn round_trips_through_private_bytes() {
        let a = EphemeralKeyPair::generate();
        let b = EphemeralKeyPair::generate();
        let restored = EphemeralKeyPair::from_private_bytes(&a.private_bytes()).unwrap();
        assert_eq!(restored.public_bytes(), a.public_bytes());
        // The restored handle agrees to the same secret as the original.
        assert_eq!(restored.agree(&b.public_bytes()).unwrap(), a.agree(&b.public_bytes()).unwrap());
    }

    #[test]
    fn different_peers_differ() {
        let a = EphemeralKeyPair::generate();
        let b = EphemeralKeyPair::generate();
        let c = EphemeralKeyPair::generate();
        assert_ne!(a.agree(&b.public_bytes()).unwrap(), a.agree(&c.public_bytes()).unwrap());
    }

    #[test]
    fn bad_public_bytes_rejected() {
        let a = EphemeralKeyPair::generate();
        assert!(a.agree(b"not a public key").is_err());
        assert!(a.agree(&[]).is_err());
    }
}
