//! Device-linking provisioning channel (`docs/04-multi-device.md` §4).
//!
//! Linking transports the identity's *static credential* (shared identity key,
//! rotation key, storage key, routing info) onto a new device without the
//! server ever seeing plaintext, then lets the new device build its own
//! per-device state. The transport is a short-lived, ciphertext-only mailbox on
//! a homeserver (the `relay` here is the homeserver, not the push relay).
//!
//! This module owns the *crypto and encoding* of that flow; the orchestration
//! (mailbox round-trips, registration, storage pull) lives in `lib.rs`.
//!
//! Handshake: both devices generate ephemeral Curve25519 key pairs
//! ([`crypto::EphemeralKeyPair`]), exchange public halves — one out-of-band via
//! the pairing string (scanned QR or pasted text), the other over the mailbox —
//! and derive `K = HKDF(X25519(...))`. The existing device seals the
//! [`ProvisioningBundle`] under `K`; the new device opens it.
//!
//! Security: `K` depends on the pairing string's `ephemeral_pub`, which travels
//! only over the trusted out-of-band channel. A hostile mailbox can drop or
//! swap bytes (both sides then fail to derive a common `K` → clean abort) but
//! never learns `K`, so it cannot read the bundle. AES-GCM integrity then
//! authenticates the bundle as coming from a holder of `K`.
//!
//! Wire format of the sealed bundle mirrors the recovery blob:
//!   `version(1) || nonce(12) || AES-256-GCM(ProvisioningBundle proto || tag)`

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::prelude::*;
use prost::Message as _;

use crate::error::AppError;

/// Public re-export so callers don't reach into `crate::proto`.
pub use crate::proto::provisioning::ProvisioningBundle;

const NONCE_LEN: usize = 12;

/// Sealed-bundle wire-format version. Bump on any format change.
pub const PROVISIONING_BUNDLE_VERSION: u8 = 1;

/// HKDF label binding the derived key to this purpose/version.
const HKDF_LABEL_PROVISIONING: &[u8] = b"actnet-provisioning-v1";

/// Default ephemeral-mailbox server, used **only** when a server-less device is
/// the one showing the pairing code (an already-provisioned device always hosts
/// the mailbox on its own server). The mailbox holds opaque ciphertext for a
/// few minutes; the identity's real home server arrives inside the bundle. This
/// is overridable so a deployment can avoid leaking rendezvous metadata to a
/// third party. See [[demo-homeserver]] for why this host is used in dev.
pub const DEFAULT_MAILBOX_SERVER: &str = "https://av.theavalanche.net";

/// Mailbox slot names. The scanner posts its ephemeral pubkey to `handshake`;
/// the existing device posts the sealed bundle to `bundle`.
pub const SLOT_HANDSHAKE: &str = "handshake";
pub const SLOT_BUNDLE: &str = "bundle";

/// Derive the AES-256 key from the raw 32-byte X25519 shared secret.
pub fn derive_shared_key(ecdh_secret: &[u8; 32]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, ecdh_secret);
    let mut key = [0u8; 32];
    hk.expand(HKDF_LABEL_PROVISIONING, &mut key)
        .expect("HKDF expand never fails for 32-byte output");
    key
}

/// Seal a provisioning bundle under the derived shared key.
pub fn seal_bundle(bundle: &ProvisioningBundle, shared_key: &[u8; 32]) -> Result<Vec<u8>, AppError> {
    let body = bundle.encode_to_vec();

    let cipher = Aes256Gcm::new(shared_key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, body.as_slice())
        .map_err(|e| AppError::Provisioning(format!("bundle encryption failed: {e}")))?;

    let mut blob = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    blob.push(PROVISIONING_BUNDLE_VERSION);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Open a sealed provisioning bundle with the derived shared key.
pub fn open_bundle(blob: &[u8], shared_key: &[u8; 32]) -> Result<ProvisioningBundle, AppError> {
    if blob.len() < 1 + NONCE_LEN + 16 {
        return Err(AppError::Provisioning("provisioning bundle too short".into()));
    }

    let version = blob[0];
    if version != PROVISIONING_BUNDLE_VERSION {
        return Err(AppError::Provisioning(format!(
            "unsupported provisioning bundle version: {version} (expected {PROVISIONING_BUNDLE_VERSION})"
        )));
    }

    let (nonce_bytes, ciphertext) = blob[1..].split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(shared_key.into());
    let body = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Provisioning("bundle decryption failed (wrong key?)".into()))?;

    ProvisioningBundle::decode(body.as_slice())
        .map_err(|e| AppError::Provisioning(format!("bundle proto parse failed: {e}")))
}

/// The out-of-band pairing payload carried by the QR / pasted code. Emitted by
/// the device that *shows* the code; ingested by the device that scans/pastes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// Origin of the homeserver hosting the ephemeral mailbox (no trailing slash).
    pub mailbox_url: String,
    /// Mailbox session id allocated by that server.
    pub session_id: String,
    /// The shower's ephemeral Curve25519 public key (serialized, 33 bytes).
    pub ephemeral_pub: Vec<u8>,
}

/// Pairing-string format tag. The whole string is URL-safe (base64url, no pad,
/// `.`-separated) so it round-trips through both a QR code and a copy-paste.
const PAIRING_PREFIX: &str = "av1";

impl PairingCode {
    /// Encode to the canonical pairing string (`av1.<b64url x3>`).
    pub fn encode(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            PAIRING_PREFIX,
            BASE64_URL_SAFE_NO_PAD.encode(self.mailbox_url.as_bytes()),
            BASE64_URL_SAFE_NO_PAD.encode(self.session_id.as_bytes()),
            BASE64_URL_SAFE_NO_PAD.encode(&self.ephemeral_pub),
        )
    }

    /// Parse a canonical pairing string (whitespace-trimmed). Rejects a wrong
    /// prefix/version, wrong field count, or non-base64url/non-UTF-8 fields.
    pub fn decode(s: &str) -> Result<Self, AppError> {
        let parts: Vec<&str> = s.trim().split('.').collect();
        if parts.len() != 4 || parts[0] != PAIRING_PREFIX {
            return Err(AppError::Provisioning("invalid pairing code".into()));
        }
        let bad = |what: &str| AppError::Provisioning(format!("invalid pairing code ({what})"));
        let mailbox_url = String::from_utf8(
            BASE64_URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| bad("mailbox_url"))?,
        )
        .map_err(|_| bad("mailbox_url utf8"))?;
        let session_id = String::from_utf8(
            BASE64_URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| bad("session_id"))?,
        )
        .map_err(|_| bad("session_id utf8"))?;
        let ephemeral_pub = BASE64_URL_SAFE_NO_PAD
            .decode(parts[3])
            .map_err(|_| bad("ephemeral_pub"))?;
        Ok(Self { mailbox_url, session_id, ephemeral_pub })
    }
}

/// In-progress link handshake state, held between the pairing-code step and the
/// bundle step. Used on both the existing device (`AppCore::link_state`) and the
/// new device (`DeviceLinkNew`). Role-flexible: either device can be the one
/// that shows the code or the one that scans it.
#[derive(Clone)]
pub struct LinkHandshake {
    /// This device's ephemeral key pair (byte-backed; `Send + Sync`).
    pub ephemeral: crypto::EphemeralKeyPair,
    /// Mailbox host (origin) both devices rendezvous through.
    pub mailbox_url: String,
    /// Mailbox session id.
    pub session_id: String,
    /// The peer's ephemeral public key. `Some` if we scanned the peer's code
    /// (the key came from the code); `None` if we showed the code (the peer is
    /// the scanner, and its key must be fetched from the `handshake` slot).
    pub peer_pub: Option<Vec<u8>>,
}

impl LinkHandshake {
    /// Derive the AES key shared with the peer, given the peer's ephemeral
    /// public key.
    pub fn shared_key(&self, peer_pub: &[u8]) -> Result<[u8; 32], AppError> {
        let secret = self.ephemeral.agree(peer_pub).map_err(AppError::from)?;
        Ok(derive_shared_key(&secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bundle() -> ProvisioningBundle {
        ProvisioningBundle {
            identity_keypair: b"fake-identity-keypair".to_vec(),
            rotation_key_private: vec![7u8; 32],
            storage_key: vec![9u8; 32],
            did: "did:plc:abc123".into(),
            servers: vec!["https://hs-a".into(), "https://hs-b".into()],
            display_name: "Sam".into(),
            profile_key: vec![3u8; 32],
            new_device_id: 3,
            link_nonce: "test-nonce".into(),
        }
    }

    #[test]
    fn bundle_round_trip() {
        let key = [42u8; 32];
        let b = sample_bundle();
        let sealed = seal_bundle(&b, &key).unwrap();
        assert_eq!(sealed[0], PROVISIONING_BUNDLE_VERSION);
        let opened = open_bundle(&sealed, &key).unwrap();
        assert_eq!(opened, b);
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal_bundle(&sample_bundle(), &[1u8; 32]).unwrap();
        assert!(open_bundle(&sealed, &[2u8; 32]).is_err());
    }

    #[test]
    fn tamper_fails() {
        let key = [42u8; 32];
        let mut sealed = seal_bundle(&sample_bundle(), &key).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(open_bundle(&sealed, &key).is_err());
    }

    #[test]
    fn wrong_version_rejected() {
        let key = [42u8; 32];
        let mut sealed = seal_bundle(&sample_bundle(), &key).unwrap();
        sealed[0] = 0xFF;
        assert!(open_bundle(&sealed, &key).is_err());
    }

    #[test]
    fn derive_shared_key_matches_for_agreeing_peers() {
        let a = crypto::EphemeralKeyPair::generate();
        let b = crypto::EphemeralKeyPair::generate();
        let ka = derive_shared_key(&a.agree(&b.public_bytes()).unwrap());
        let kb = derive_shared_key(&b.agree(&a.public_bytes()).unwrap());
        assert_eq!(ka, kb);
        // End-to-end: seal with one side's key, open with the other's.
        let sealed = seal_bundle(&sample_bundle(), &ka).unwrap();
        assert_eq!(open_bundle(&sealed, &kb).unwrap(), sample_bundle());
    }

    #[test]
    fn pairing_code_round_trip() {
        let pc = PairingCode {
            mailbox_url: "https://av.theavalanche.net".into(),
            session_id: "01J9X-session-uuid".into(),
            ephemeral_pub: vec![5u8; 33],
        };
        let s = pc.encode();
        assert!(s.starts_with("av1."));
        assert_eq!(PairingCode::decode(&s).unwrap(), pc);
        // Tolerates surrounding whitespace (pasted codes).
        assert_eq!(PairingCode::decode(&format!("  {s}\n")).unwrap(), pc);
    }

    #[test]
    fn pairing_code_rejects_garbage() {
        assert!(PairingCode::decode("").is_err());
        assert!(PairingCode::decode("av1.only.two").is_err());
        assert!(PairingCode::decode("av2.a.b.c").is_err());
        assert!(PairingCode::decode("av1.!!!.b.c").is_err());
    }

    use proptest::prelude::*;

    proptest! {
        /// Sealing then opening any bundle payload with the same key round-trips,
        /// and opening with any different key fails (never silently wrong).
        #[test]
        fn prop_seal_open_round_trip(
            id in prop::collection::vec(any::<u8>(), 0..200),
            rot in prop::collection::vec(any::<u8>(), 0..64),
            did in "[a-z:0-9]{0,40}",
            key in any::<[u8; 32]>(),
            other in any::<[u8; 32]>(),
        ) {
            let bundle = ProvisioningBundle {
                identity_keypair: id,
                rotation_key_private: rot,
                storage_key: vec![1u8; 32],
                did,
                servers: vec!["https://hs".into()],
                display_name: String::new(),
                profile_key: vec![],
                new_device_id: 2,
                link_nonce: "n".into(),
            };
            let sealed = seal_bundle(&bundle, &key).unwrap();
            prop_assert_eq!(open_bundle(&sealed, &key).unwrap(), bundle);
            if other != key {
                prop_assert!(open_bundle(&sealed, &other).is_err());
            }
        }

        /// Any pairing payload encodes to a string that decodes back identically.
        #[test]
        fn prop_pairing_code_round_trip(
            url in "[ -~]{0,60}",
            session in "[ -~]{0,60}",
            pubk in prop::collection::vec(any::<u8>(), 0..40),
        ) {
            let pc = PairingCode { mailbox_url: url, session_id: session, ephemeral_pub: pubk };
            prop_assert_eq!(PairingCode::decode(&pc.encode()).unwrap(), pc);
        }
    }
}
