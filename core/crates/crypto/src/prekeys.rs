//! Prekey generation and wire-format types.
//!
//! Signal's X3DH key-agreement protocol requires each user to publish a set of
//! prekeys to the server. When Alice wants to message Bob who is offline, she
//! fetches Bob's prekey bundle and uses it to derive a shared secret without
//! Bob's involvement. Bob's device consumes the prekey when it next comes
//! online and processes Alice's first message.
//!
//! This module handles three kinds of prekeys:
//!
//! - **Signed EC prekeys** — medium-term keys signed by the identity key,
//!   rotated periodically (typically weekly).
//! - **One-time EC prekeys** — ephemeral keys consumed one-per-session; their
//!   pool is refilled whenever it runs low.
//! - **Kyber prekeys** — post-quantum ML-KEM-1024 keys, also signed by the
//!   identity key, providing resistance to "harvest now, decrypt later" attacks
//!   by quantum adversaries.
//!
//! Each `generate_*` function returns both the wire format (for uploading to
//! the server) and the serialized record bytes (for persisting in the local
//! store). The caller — `app-core` — is responsible for saving the record and
//! deciding when to upload the wire format.

use libsignal_protocol::{self as signal, GenericSignedPreKey as _};
use std::time::SystemTime;

// See session.rs for explanation of the `TryRngCore::unwrap_err()` pattern.
use rand::TryRngCore as _;

use crate::{error::CryptoError, identity::IdentityKeyPair};

// ── Wire-format types (sent to/received from the homeserver) ──────────────────

/// A signed EC prekey in wire format.
#[derive(Debug, Clone)]
pub struct SignedPreKey {
    pub id: u32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

/// A one-time EC prekey in wire format.
#[derive(Debug, Clone)]
pub struct OneTimePreKey {
    pub id: u32,
    pub public_key: Vec<u8>,
}

/// A signed Kyber (post-quantum) prekey in wire format.
#[derive(Debug, Clone)]
pub struct KyberPreKey {
    pub id: u32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

/// What we publish to the server on account registration or prekey refresh.
#[derive(Debug)]
pub struct LocalKeyBundle {
    pub identity_key: Vec<u8>,
    pub signed_prekey: SignedPreKey,
    pub one_time_prekeys: Vec<OneTimePreKey>,
    pub kyber_prekey: KyberPreKey,
}

/// What we fetch from the server when initiating a session with another user.
#[derive(Debug)]
pub struct RecipientKeyBundle {
    pub identity_key: Vec<u8>,
    pub registration_id: u32,
    pub device_id: u32,
    pub signed_prekey: SignedPreKey,
    /// Absent if the recipient's one-time prekey pool was empty.
    pub one_time_prekey: Option<OneTimePreKey>,
    pub kyber_prekey: KyberPreKey,
}

// ── Generated types (include the full record for store persistence) ───────────

/// A generated signed EC prekey.
pub struct GeneratedSignedPreKey {
    pub wire: SignedPreKey,
    /// Serialized `SignedPreKeyRecord` for the local store.
    pub record: Vec<u8>,
}

/// A generated one-time EC prekey.
pub struct GeneratedOneTimePreKey {
    pub wire: OneTimePreKey,
    /// Serialized `PreKeyRecord` for the local store.
    pub record: Vec<u8>,
}

/// A generated signed Kyber prekey.
pub struct GeneratedKyberPreKey {
    pub wire: KyberPreKey,
    /// Serialized `KyberPreKeyRecord` for the local store.
    pub record: Vec<u8>,
}

// ── Generation ────────────────────────────────────────────────────────────────

pub fn generate_signed_prekey(
    identity: &IdentityKeyPair,
    id: u32,
) -> Result<GeneratedSignedPreKey, CryptoError> {
    let mut rng = rand::rngs::OsRng.unwrap_err();
    let key_pair = signal::KeyPair::generate(&mut rng);
    let public_key_bytes = key_pair.public_key.serialize().to_vec();

    let signature = identity
        .0
        .private_key()
        .calculate_signature(&public_key_bytes, &mut rng)
        .map_err(|e| CryptoError::Signal(e.into()))?
        .to_vec();

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_millis() as u64;

    let record = signal::SignedPreKeyRecord::new(
        signal::SignedPreKeyId::from(id),
        signal::Timestamp::from_epoch_millis(timestamp),
        &key_pair,
        &signature,
    );

    Ok(GeneratedSignedPreKey {
        wire: SignedPreKey {
            id,
            public_key: public_key_bytes,
            signature,
        },
        record: record.serialize().map_err(CryptoError::Signal)?.to_vec(),
    })
}

pub fn generate_one_time_prekeys(
    start_id: u32,
    count: usize,
) -> Result<Vec<GeneratedOneTimePreKey>, CryptoError> {
    let mut rng = rand::rngs::OsRng.unwrap_err();
    (start_id..)
        .take(count)
        .map(|id| {
            let key_pair = signal::KeyPair::generate(&mut rng);
            let public_key_bytes = key_pair.public_key.serialize().to_vec();
            let record = signal::PreKeyRecord::new(signal::PreKeyId::from(id), &key_pair);
            Ok(GeneratedOneTimePreKey {
                wire: OneTimePreKey {
                    id,
                    public_key: public_key_bytes,
                },
                record: record.serialize().map_err(CryptoError::Signal)?.to_vec(),
            })
        })
        .collect()
}

pub fn generate_kyber_prekey(
    identity: &IdentityKeyPair,
    id: u32,
) -> Result<GeneratedKyberPreKey, CryptoError> {
    let record = signal::KyberPreKeyRecord::generate(
        signal::kem::KeyType::Kyber1024,
        signal::KyberPreKeyId::from(id),
        identity.0.private_key(),
    )
    .map_err(CryptoError::Signal)?;

    let public_key_bytes = record
        .public_key()
        .map_err(CryptoError::Signal)?
        .serialize()
        .to_vec();

    let signature = record
        .signature()
        .map_err(CryptoError::Signal)?
        .to_vec();

    Ok(GeneratedKyberPreKey {
        wire: KyberPreKey {
            id,
            public_key: public_key_bytes,
            signature,
        },
        record: record.serialize().map_err(CryptoError::Signal)?.to_vec(),
    })
}

// ── Conversion helpers ────────────────────────────────────────────────────────

impl RecipientKeyBundle {
    /// Convert to a libsignal `PreKeyBundle` for use with `process_prekey_bundle`.
    pub fn to_signal_bundle(&self) -> Result<signal::PreKeyBundle, CryptoError> {
        let identity_key =
            signal::IdentityKey::decode(&self.identity_key).map_err(CryptoError::Signal)?;

        let device_id = signal::DeviceId::try_from(self.device_id)
            .map_err(|_| CryptoError::InvalidKey)?;

        let signed_prekey_public =
            signal::PublicKey::deserialize(&self.signed_prekey.public_key)
                .map_err(|e| CryptoError::Signal(e.into()))?;

        let ec_prekey = match &self.one_time_prekey {
            Some(otpk) => {
                let pk = signal::PublicKey::deserialize(&otpk.public_key)
                    .map_err(|e| CryptoError::Signal(e.into()))?;
                Some((signal::PreKeyId::from(otpk.id), pk))
            }
            None => None,
        };

        let kyber_public = signal::kem::PublicKey::deserialize(&self.kyber_prekey.public_key)
            .map_err(CryptoError::Signal)?;

        signal::PreKeyBundle::new(
            self.registration_id,
            device_id,
            ec_prekey,
            signal::SignedPreKeyId::from(self.signed_prekey.id),
            signed_prekey_public,
            self.signed_prekey.signature.clone(),
            signal::KyberPreKeyId::from(self.kyber_prekey.id),
            kyber_public,
            self.kyber_prekey.signature.clone(),
            identity_key,
        )
        .map_err(CryptoError::Signal)
    }
}
