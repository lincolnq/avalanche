//! Encrypted profile blob format and helpers.
//!
//! The profile is serialized as JSON, encrypted with the 32-byte profile key
//! using AES-256-GCM, and uploaded to the homeserver as opaque bytes. Only
//! contacts who hold the profile key can decrypt.
//!
//! Wire format: `nonce (12 bytes) || ciphertext (includes 16-byte GCM tag)` —
//! the same layout used for recovery blobs.
//!
//! For stage 4 the JSON object contains only `display_name`. Future avatar
//! and bio fields are added to the same object; older clients ignore unknown
//! fields. No schema version field is needed.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const NONCE_LEN: usize = 12;
/// Profile keys are 32 bytes (AES-256).
pub const PROFILE_KEY_LEN: usize = 32;

/// Plaintext contents of an encrypted profile blob. Unknown fields are
/// preserved on deserialize (via `#[serde(default)]`) but we don't emit
/// fields we don't know about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilePlaintext {
    pub display_name: String,
}

/// Generate a fresh 32-byte profile key.
pub fn generate_profile_key() -> [u8; PROFILE_KEY_LEN] {
    rand::Rng::random(&mut rand::rng())
}

/// Encrypt a profile plaintext under the given 32-byte profile key.
pub fn encrypt_profile(
    plaintext: &ProfilePlaintext,
    profile_key: &[u8; PROFILE_KEY_LEN],
) -> Result<Vec<u8>, AppError> {
    let json = serde_json::to_vec(plaintext)
        .map_err(|e| AppError::Protocol(format!("failed to serialize profile: {e}")))?;

    let cipher = Aes256Gcm::new(profile_key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, json.as_slice())
        .map_err(|e| AppError::Protocol(format!("profile encryption failed: {e}")))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt a profile blob with the given 32-byte profile key.
pub fn decrypt_profile(
    blob: &[u8],
    profile_key: &[u8; PROFILE_KEY_LEN],
) -> Result<ProfilePlaintext, AppError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(AppError::Protocol("profile blob too short".into()));
    }

    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(profile_key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Protocol("profile decryption failed (wrong key?)".into()))?;

    serde_json::from_slice(&plaintext)
        .map_err(|e| AppError::Protocol(format!("profile JSON parse failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = generate_profile_key();
        let plaintext = ProfilePlaintext { display_name: "Alice".into() };
        let blob = encrypt_profile(&plaintext, &key).unwrap();
        let decrypted = decrypt_profile(&blob, &key).unwrap();
        assert_eq!(decrypted.display_name, "Alice");
    }

    #[test]
    fn wrong_key_fails() {
        let key = generate_profile_key();
        let other = generate_profile_key();
        let plaintext = ProfilePlaintext { display_name: "Alice".into() };
        let blob = encrypt_profile(&plaintext, &key).unwrap();
        assert!(decrypt_profile(&blob, &other).is_err());
    }
}
