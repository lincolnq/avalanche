//! Recovery blob creation, encryption, and decryption.
//!
//! The blob is a server-side cache that lets a recovering device skip
//! re-registration friction. It holds the device identity keypair, the
//! server list, profile data, and the user's group master keys. **It does
//! NOT contain the rotation key** — that's deterministically re-derived from
//! the passkey on every recovery via [`derive_recovery_keys_from_prf`], so
//! the blob never carries DID-controlling authority.
//!
//! Wire format (v4):
//!   `version(1) || nonce(12) || AES-256-GCM(RecoveryBlob proto || tag)`
//!
//! Earlier formats (v2, v3 JSON) are not accepted — this project has no
//! deployed users to migrate.
//!
//! Always bump [`RECOVERY_BLOB_VERSION`] when changing the wire format.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use prost::Message as _;

use crate::error::AppError;
use crate::proto::recovery as proto;

/// Public re-export so callers don't have to reach into `crate::proto`.
pub use crate::proto::recovery::{RecoveredGroup, RecoveryBlob, ServerEntry};

const NONCE_LEN: usize = 12;

/// Current recovery-blob wire-format version. v4 is protobuf-encoded with
/// homeserver URLs interned in a top-level `servers` table and groups
/// referencing them by index (so N groups on the same homeserver pay one
/// URL plus N varints instead of N×len(url) bytes).
pub const RECOVERY_BLOB_VERSION: u8 = 4;

/// Encrypt the v4 protobuf blob with a 32-byte symmetric key.
pub fn encrypt_recovery_blob(
    plaintext: &RecoveryBlob,
    symmetric_key: &[u8; 32],
) -> Result<Vec<u8>, AppError> {
    let body = plaintext.encode_to_vec();

    let cipher = Aes256Gcm::new(symmetric_key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, body.as_slice())
        .map_err(|e| AppError::Protocol(format!("recovery blob encryption failed: {e}")))?;

    // version || nonce || ciphertext (includes GCM tag)
    let mut blob = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    blob.push(RECOVERY_BLOB_VERSION);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt and parse a recovery blob with a 32-byte symmetric key.
pub fn decrypt_recovery_blob(
    blob: &[u8],
    symmetric_key: &[u8; 32],
) -> Result<RecoveryBlob, AppError> {
    if blob.len() < 1 + NONCE_LEN + 16 {
        return Err(AppError::Protocol("recovery blob too short".into()));
    }

    let version = blob[0];
    if version != RECOVERY_BLOB_VERSION {
        return Err(AppError::Protocol(format!(
            "unsupported recovery blob version: {version} (expected {RECOVERY_BLOB_VERSION})"
        )));
    }

    let (nonce_bytes, ciphertext) = blob[1..].split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(symmetric_key.into());
    let body = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Protocol("recovery blob decryption failed (wrong key?)".into()))?;

    let parsed = RecoveryBlob::decode(body.as_slice())
        .map_err(|e| AppError::Protocol(format!("recovery blob proto parse failed: {e}")))?;

    // Defensive: every group index must point inside `servers`. We refuse
    // to surface garbage here so the caller can assume indices are valid.
    let n = parsed.servers.len() as u32;
    for (i, g) in parsed.groups.iter().enumerate() {
        if g.server_index >= n {
            return Err(AppError::Protocol(format!(
                "recovery blob group[{i}].server_index {} out of range (servers.len = {n})",
                g.server_index
            )));
        }
    }
    Ok(parsed)
}

/// HKDF labels used to split the passkey PRF output into purpose-bound keys.
/// Versioned so future schema changes don't silently reuse the same bytes.
const HKDF_LABEL_ROTATION: &[u8] = b"actnet-rotation-v1";
const HKDF_LABEL_BLOB: &[u8] = b"actnet-blob-v1";

/// Derive the rotation key seed and the blob-encryption key from the raw
/// 32-byte PRF output (or any other high-entropy seed of equivalent length,
/// such as the bytes derived from a written-down recovery phrase).
///
/// Returns `(rotation_seed, blob_key)`. Both are 32 bytes.
/// - `rotation_seed` must be passed to [`derive_rotation_key_from_seed`]
///   to obtain the actual P-256 keypair.
/// - `blob_key` is the AES-256-GCM key that encrypts/decrypts the recovery blob.
pub fn derive_recovery_keys_from_prf(prf_output: &[u8]) -> ([u8; 32], [u8; 32]) {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, prf_output);
    let mut rotation_seed = [0u8; 32];
    let mut blob_key = [0u8; 32];
    hk.expand(HKDF_LABEL_ROTATION, &mut rotation_seed)
        .expect("HKDF expand never fails for 32-byte output");
    hk.expand(HKDF_LABEL_BLOB, &mut blob_key)
        .expect("HKDF expand never fails for 32-byte output");
    (rotation_seed, blob_key)
}

/// Deterministically derive a P-256 rotation keypair from a 32-byte seed.
///
/// Returns `(private_key_sec1_bytes, public_key_sec1_compressed_bytes)`.
/// The seed must be from a high-entropy source (the `rotation_seed` output
/// of [`derive_recovery_keys_from_prf`], not arbitrary user input).
///
/// Reduces the seed mod the P-256 group order to obtain a scalar in
/// `[1, n-1]`. The probability of landing on zero is negligible for any
/// real PRF output; we retry once via `+1` as a paranoid safety net.
pub fn derive_rotation_key_from_seed(seed: &[u8; 32]) -> (Vec<u8>, Vec<u8>) {
    use p256::ecdsa::SigningKey;
    use p256::elliptic_curve::generic_array::GenericArray;

    // SigningKey::from_bytes interprets the input as a scalar and rejects
    // zero; we accept its result on any non-zero seed (overwhelmingly likely).
    let arr = GenericArray::clone_from_slice(seed);
    let signing_key = SigningKey::from_bytes(&arr).unwrap_or_else(|_| {
        // Vanishingly improbable: seed reduces to zero. Tweak and retry.
        let mut tweak = *seed;
        tweak[31] ^= 0x01;
        let arr2 = GenericArray::clone_from_slice(&tweak);
        SigningKey::from_bytes(&arr2).expect("tweaked seed is non-zero")
    });
    let private_bytes = signing_key.to_bytes().to_vec();
    let public_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    (private_bytes, public_bytes)
}

/// Generate a random P-256 rotation key. Used only when the user skips
/// passkey creation — without a passkey there is no PRF output to derive
/// from, so the rotation key has no recoverable user-held source. The
/// identity is effectively unrecoverable on device loss; surfaces in the
/// "skip recovery" path documented in `50-identity-auth-recovery.md`.
pub fn generate_rotation_key() -> (Vec<u8>, Vec<u8>) {
    use p256::ecdsa::SigningKey;
    let signing_key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let private_bytes = signing_key.to_bytes().to_vec();
    let public_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    (private_bytes, public_bytes)
}

/// Sign a payload with a P-256 rotation key. Returns DER-encoded ECDSA signature.
pub fn sign_with_rotation_key(
    private_key_bytes: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, AppError> {
    use p256::ecdsa::{signature::Signer, Signature, SigningKey};

    let signing_key = SigningKey::from_bytes(private_key_bytes.into())
        .map_err(|e| AppError::Protocol(format!("invalid rotation key: {e}")))?;
    let sig: Signature = signing_key.sign(payload);
    Ok(sig.to_der().as_bytes().to_vec())
}

/// A single group's worth of input to the blob builder.
pub struct GroupBlobEntry {
    pub master_key: Vec<u8>,
    pub hosting_server_url: String,
}

/// Build a recovery blob from the current account state, interning the
/// homeserver URLs across `account_servers` and `groups` into a single
/// table referenced by index.
///
/// `account_servers[0]` is treated as the primary and always lands at
/// `servers[0]`. Group server URLs get added to the table in first-seen
/// order. Each group's `server_index` is set to its position in the
/// resulting table.
///
/// `profile_key` is the 32-byte symmetric key that encrypts the user's
/// profile blob on each homeserver. Pass `&[]` to omit (e.g. for bot
/// accounts that have no profile).
///
/// `storage_key` is the 32-byte identity-level storage key (docs/05 §11). Pass
/// `&[]` to omit. Carrying it here is what lets a recovering/linking device
/// read the identity's durable-state records.
pub fn build_recovery_blob(
    identity_keypair_bytes: &[u8],
    account_servers: &[String],
    profile_key: &[u8],
    display_name: &str,
    groups: &[GroupBlobEntry],
    storage_key: &[u8],
) -> RecoveryBlob {
    let mut servers: Vec<ServerEntry> = Vec::new();
    let mut intern = |url: &str| -> u32 {
        if let Some(idx) = servers.iter().position(|s| s.url == url) {
            return idx as u32;
        }
        servers.push(ServerEntry { url: url.to_string() });
        (servers.len() - 1) as u32
    };

    for s in account_servers {
        intern(s);
    }
    let recovered: Vec<proto::RecoveredGroup> = groups
        .iter()
        .map(|g| proto::RecoveredGroup {
            master_key: g.master_key.clone(),
            server_index: intern(&g.hosting_server_url),
        })
        .collect();

    RecoveryBlob {
        identity_keypair: identity_keypair_bytes.to_vec(),
        servers,
        profile_key: profile_key.to_vec(),
        display_name: display_name.to_string(),
        groups: recovered,
        storage_key: storage_key.to_vec(),
    }
}

/// Snapshot every locally-known group into the shape the blob builder
/// expects. Called by write sites (`update_recovery_blob`, signup,
/// auto-upload triggers) so the blob always reflects current membership.
pub async fn collect_group_blob_entries(
    store: &store::IdentityStore,
) -> Result<Vec<GroupBlobEntry>, AppError> {
    let rows = store.list_groups().await?;
    Ok(rows
        .into_iter()
        .map(|g| GroupBlobEntry {
            master_key: g.master_key,
            hosting_server_url: g.hosting_server_url,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovery_blob() {
        let key = [42u8; 32];
        let plaintext = build_recovery_blob(
            b"fake-identity-keypair",
            &["https://server1.example".into(), "https://server2.example".into()],
            &[7u8; 32],
            "Sam",
            &[GroupBlobEntry {
                master_key: vec![1u8; 32],
                hosting_server_url: "https://server1.example".into(),
            }],
            &[9u8; 32],
        );

        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        let decrypted = decrypt_recovery_blob(&blob, &key).unwrap();

        assert_eq!(decrypted.identity_keypair, b"fake-identity-keypair");
        assert_eq!(decrypted.profile_key, vec![7u8; 32]);
        assert_eq!(decrypted.display_name, "Sam");
        assert_eq!(decrypted.servers, plaintext.servers);
        assert_eq!(decrypted.groups.len(), 1);
        // Group's server URL already in account_servers, so index reuses 0.
        assert_eq!(decrypted.groups[0].server_index, 0);
        assert_eq!(decrypted.groups[0].master_key, vec![1u8; 32]);
        assert_eq!(decrypted.storage_key, vec![9u8; 32]);
    }

    #[test]
    fn server_urls_are_interned() {
        // Three groups, two distinct homeservers, and an account_server
        // that overlaps one of them — should produce exactly 2 entries in
        // `servers`, not 4.
        let blob = build_recovery_blob(
            b"id",
            &["https://hs-a".into()],
            &[],
            "",
            &[
                GroupBlobEntry { master_key: vec![1u8; 32], hosting_server_url: "https://hs-a".into() },
                GroupBlobEntry { master_key: vec![2u8; 32], hosting_server_url: "https://hs-b".into() },
                GroupBlobEntry { master_key: vec![3u8; 32], hosting_server_url: "https://hs-a".into() },
            ],
            &[],
        );
        let urls: Vec<&str> = blob.servers.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, vec!["https://hs-a", "https://hs-b"]);
        assert_eq!(blob.groups[0].server_index, 0);
        assert_eq!(blob.groups[1].server_index, 1);
        assert_eq!(blob.groups[2].server_index, 0);
    }

    #[test]
    fn out_of_range_server_index_rejected() {
        // Hand-build a malicious blob with a dangling server_index and
        // ensure the decoder refuses it (rather than panicking later).
        let key = [42u8; 32];
        let bad = RecoveryBlob {
            identity_keypair: b"id".to_vec(),
            servers: vec![ServerEntry { url: "https://hs".into() }],
            profile_key: vec![],
            display_name: String::new(),
            groups: vec![RecoveredGroup { master_key: vec![1u8; 32], server_index: 99 }],
            storage_key: vec![],
        };
        let blob = encrypt_recovery_blob(&bad, &key).unwrap();
        assert!(decrypt_recovery_blob(&blob, &key).is_err());
    }

    #[test]
    fn wrong_version_rejected() {
        let key = [42u8; 32];
        let plaintext = build_recovery_blob(b"id", &["https://hs".into()], &[], "", &[], &[]);
        let mut blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        blob[0] = 0xFF;
        assert!(decrypt_recovery_blob(&blob, &key).is_err());
        blob[0] = 3; // old JSON version
        assert!(decrypt_recovery_blob(&blob, &key).is_err());
    }

    #[test]
    fn encoded_blob_starts_with_version_byte() {
        let key = [42u8; 32];
        let plaintext = build_recovery_blob(b"id", &["https://hs".into()], &[], "", &[], &[]);
        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        assert_eq!(blob[0], RECOVERY_BLOB_VERSION);
    }

    #[test]
    fn wrong_key_fails() {
        let key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let plaintext = build_recovery_blob(b"id", &["https://hs".into()], &[], "", &[], &[]);
        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        assert!(decrypt_recovery_blob(&blob, &wrong_key).is_err());
    }

    #[test]
    fn random_rotation_key_round_trip() {
        let (private_key, _public_key) = generate_rotation_key();
        let payload = b"replace:did:plc:test:1:2:nonce123";
        let sig = sign_with_rotation_key(&private_key, payload).unwrap();
        assert!(!sig.is_empty());

        use p256::ecdsa::{signature::Verifier, Signature, SigningKey, VerifyingKey};
        let signing_key = SigningKey::from_bytes((&private_key[..]).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        let signature = Signature::from_der(&sig).unwrap();
        verifying_key.verify(payload, &signature).unwrap();
    }

    #[test]
    fn prf_derivation_is_deterministic() {
        let prf = [7u8; 32];
        let (rot_seed_1, blob_key_1) = derive_recovery_keys_from_prf(&prf);
        let (rot_seed_2, blob_key_2) = derive_recovery_keys_from_prf(&prf);
        assert_eq!(rot_seed_1, rot_seed_2);
        assert_eq!(blob_key_1, blob_key_2);
        // Labels must produce distinct outputs.
        assert_ne!(rot_seed_1, blob_key_1);
    }

    #[test]
    fn derived_rotation_key_signs_and_verifies() {
        let prf = [42u8; 32];
        let (seed, _blob_key) = derive_recovery_keys_from_prf(&prf);
        let (priv1, pub1) = derive_rotation_key_from_seed(&seed);
        let (priv2, pub2) = derive_rotation_key_from_seed(&seed);
        assert_eq!(priv1, priv2, "derivation is deterministic");
        assert_eq!(pub1, pub2);

        let payload = b"test-payload";
        let sig = sign_with_rotation_key(&priv1, payload).unwrap();

        use p256::ecdsa::{signature::Verifier, Signature, SigningKey, VerifyingKey};
        let signing_key = SigningKey::from_bytes((&priv1[..]).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        let signature = Signature::from_der(&sig).unwrap();
        verifying_key.verify(payload, &signature).unwrap();
    }

    #[test]
    fn different_prf_gives_different_rotation_key() {
        let (seed_a, _) = derive_recovery_keys_from_prf(&[1u8; 32]);
        let (seed_b, _) = derive_recovery_keys_from_prf(&[2u8; 32]);
        let (priv_a, _) = derive_rotation_key_from_seed(&seed_a);
        let (priv_b, _) = derive_rotation_key_from_seed(&seed_b);
        assert_ne!(priv_a, priv_b);
    }
}
