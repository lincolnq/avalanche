//! Recovery blob creation, encryption, and decryption.
//!
//! The recovery blob contains the user's rotation key, identity keypair, and
//! server list — everything needed to restore an identity on a new device.
//! It is encrypted with a 32-byte symmetric key derived from the user's passkey
//! (via WebAuthn PRF extension) or recovery phrase.
//!
//! Encryption: AES-256-GCM with a random 12-byte nonce.
//! Format: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
//!
//! The plaintext is a JSON object:
//! ```json
//! {
//!   "rotation_key": "<base64 P-256 private key (SEC1/PKCS#8)>",
//!   "identity_keypair": "<base64 libsignal serialized keypair>",
//!   "servers": ["https://server1.example", "https://server2.example"]
//! }
//! ```

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const NONCE_LEN: usize = 12;

/// Plaintext contents of a recovery blob.
#[derive(Serialize, Deserialize)]
pub struct RecoveryBlobPlaintext {
    /// P-256 rotation key private scalar (SEC1 DER, base64).
    pub rotation_key: String,
    /// libsignal identity keypair (serialized bytes, base64).
    pub identity_keypair: String,
    /// List of homeserver URLs the user is registered on.
    pub servers: Vec<String>,
    /// 32-byte profile key (base64). Used to encrypt the user's display
    /// name into the server-stored profile blob. Restoring it on recovery
    /// keeps existing contacts pointed at the same profile blob, so their
    /// cached display name stays valid.
    ///
    /// `#[serde(default)]` keeps old blobs (created before this field was
    /// added) deserializable — they'll restore as `""` and the recovery
    /// flow falls back to leaving the profile unset.
    #[serde(default)]
    pub profile_key: String,
    /// User's display name in plaintext (mirrors what the server-side
    /// encrypted profile blob decrypts to). Stored locally as
    /// `own_profile.display_name`; carried in the recovery blob so a
    /// fresh device can restore it without prompting.
    ///
    /// `#[serde(default)]` keeps old blobs deserializable.
    #[serde(default)]
    pub display_name: String,
}

/// Encrypt a recovery blob with a 32-byte symmetric key.
pub fn encrypt_recovery_blob(
    plaintext: &RecoveryBlobPlaintext,
    symmetric_key: &[u8; 32],
) -> Result<Vec<u8>, AppError> {
    let json = serde_json::to_vec(plaintext)
        .map_err(|e| AppError::Protocol(format!("failed to serialize recovery blob: {e}")))?;

    let cipher = Aes256Gcm::new(symmetric_key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, json.as_slice())
        .map_err(|e| AppError::Protocol(format!("recovery blob encryption failed: {e}")))?;

    // nonce || ciphertext (includes GCM tag)
    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt a recovery blob with a 32-byte symmetric key.
pub fn decrypt_recovery_blob(
    blob: &[u8],
    symmetric_key: &[u8; 32],
) -> Result<RecoveryBlobPlaintext, AppError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(AppError::Protocol("recovery blob too short".into()));
    }

    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(symmetric_key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Protocol("recovery blob decryption failed (wrong key?)".into()))?;

    serde_json::from_slice(&plaintext)
        .map_err(|e| AppError::Protocol(format!("recovery blob JSON parse failed: {e}")))
}

/// Generate a P-256 rotation key. Returns (private_key_sec1_bytes, public_key_sec1_bytes).
pub fn generate_rotation_key() -> (Vec<u8>, Vec<u8>) {
    use p256::ecdsa::SigningKey;
    // p256 uses rand_core 0.6, not 0.9. Use p256's re-exported OsRng.
    let signing_key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let private_bytes = signing_key.to_bytes().to_vec();
    let public_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true) // compressed SEC1
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

/// Build a recovery blob plaintext from the current account state.
///
/// `profile_key` is the 32-byte symmetric key that encrypts the user's
/// profile blob on each homeserver. Pass `&[]` to omit (e.g. for bot
/// accounts that have no profile).
pub fn build_recovery_plaintext(
    rotation_key_private: &[u8],
    identity_keypair_bytes: &[u8],
    servers: &[String],
    profile_key: &[u8],
    display_name: &str,
) -> RecoveryBlobPlaintext {
    RecoveryBlobPlaintext {
        rotation_key: BASE64_STANDARD.encode(rotation_key_private),
        identity_keypair: BASE64_STANDARD.encode(identity_keypair_bytes),
        servers: servers.to_vec(),
        profile_key: if profile_key.is_empty() {
            String::new()
        } else {
            BASE64_STANDARD.encode(profile_key)
        },
        display_name: display_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovery_blob() {
        let key = [42u8; 32];
        let plaintext = RecoveryBlobPlaintext {
            rotation_key: BASE64_STANDARD.encode(b"fake-rotation-key"),
            identity_keypair: BASE64_STANDARD.encode(b"fake-identity-keypair"),
            servers: vec!["https://server1.example".into(), "https://server2.example".into()],
            profile_key: BASE64_STANDARD.encode([7u8; 32]),
            display_name: "Sam".into(),
        };

        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        let decrypted = decrypt_recovery_blob(&blob, &key).unwrap();

        assert_eq!(decrypted.rotation_key, plaintext.rotation_key);
        assert_eq!(decrypted.identity_keypair, plaintext.identity_keypair);
        assert_eq!(decrypted.servers, plaintext.servers);
        assert_eq!(decrypted.profile_key, plaintext.profile_key);
        assert_eq!(decrypted.display_name, plaintext.display_name);
    }

    #[test]
    fn old_blob_format_still_decrypts() {
        // Blobs written before profile_key/display_name fields existed should
        // still decrypt — the new fields default to "" via #[serde(default)].
        let key = [42u8; 32];
        let json = serde_json::json!({
            "rotation_key": BASE64_STANDARD.encode(b"old-rot"),
            "identity_keypair": BASE64_STANDARD.encode(b"old-identity"),
            "servers": ["https://old.example"],
        });
        let plaintext_bytes = serde_json::to_vec(&json).unwrap();

        // Hand-build the encrypted blob to simulate an old client's output.
        use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
        let cipher = Aes256Gcm::new((&key).into());
        let nonce_bytes = [0u8; NONCE_LEN];
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher.encrypt(nonce, plaintext_bytes.as_slice()).unwrap();
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ct);

        let decrypted = decrypt_recovery_blob(&blob, &key).unwrap();
        assert_eq!(decrypted.servers, vec!["https://old.example".to_string()]);
        assert_eq!(decrypted.profile_key, "");
        assert_eq!(decrypted.display_name, "");
    }

    #[test]
    fn wrong_key_fails() {
        let key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let plaintext = RecoveryBlobPlaintext {
            rotation_key: "dGVzdA==".into(),
            identity_keypair: "dGVzdA==".into(),
            servers: vec![],
            profile_key: String::new(),
            display_name: String::new(),
        };

        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        assert!(decrypt_recovery_blob(&blob, &wrong_key).is_err());
    }

    #[test]
    fn rotation_key_round_trip() {
        let (private_key, _public_key) = generate_rotation_key();
        let payload = b"replace:did:plc:test:1:2:nonce123";
        let sig = sign_with_rotation_key(&private_key, payload).unwrap();
        assert!(!sig.is_empty());

        // Verify signature
        use p256::ecdsa::{signature::Verifier, Signature, SigningKey, VerifyingKey};
        let signing_key = SigningKey::from_bytes((&private_key[..]).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        let signature = Signature::from_der(&sig).unwrap();
        verifying_key.verify(payload, &signature).unwrap();
    }
}
