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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfilePlaintext {
    pub display_name: String,
    /// Avatar version (docs/55). `None`/absent = no avatar set. Bumped on every
    /// avatar change so recipients refetch and caches bust. The image bytes live
    /// out-of-band at `avatars/account/<id>` on the discovery server, encrypted
    /// under the profile key. Older clients ignore this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_version: Option<u32>,
    /// SHA-256 of the avatar ciphertext, base64-encoded. Verified before decrypt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_digest: Option<String>,
}

/// Generate a fresh 32-byte profile key.
pub fn generate_profile_key() -> [u8; PROFILE_KEY_LEN] {
    rand::Rng::random(&mut rand::rng())
}

/// Maximum avatar *plaintext* (JPEG) size app-core will encrypt (docs/55). The
/// server caps the ciphertext at `avatar_max_size_bytes` (64 KiB default);
/// AES-256-GCM adds a 12-byte nonce + 16-byte tag, so this leaves comfortable
/// headroom below that cap. Callers (the platform UIs) downscale/compress to
/// well under this before calling `set_*_avatar`.
pub const MAX_AVATAR_BYTES: usize = 60 * 1024;

/// Encrypt raw avatar image bytes under a 32-byte key (AES-256-GCM,
/// `nonce (12) || ciphertext+tag`), the same layout as the profile blob. Used
/// for both personal avatars (encrypted under the profile key) and group
/// avatars (encrypted under a master-key-derived key). docs/55.
pub fn encrypt_avatar(
    plaintext: &[u8],
    key: &[u8; PROFILE_KEY_LEN],
) -> Result<Vec<u8>, AppError> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| AppError::Protocol(format!("avatar encryption failed: {e}")))?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt an avatar blob produced by [`encrypt_avatar`].
pub fn decrypt_avatar(blob: &[u8], key: &[u8; PROFILE_KEY_LEN]) -> Result<Vec<u8>, AppError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(AppError::Protocol("avatar blob too short".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Protocol("avatar decryption failed (wrong key?)".into()))
}

/// Base64 SHA-256 of an avatar ciphertext blob — the integrity digest carried in
/// the profile blob / group state and verified before decrypt.
pub fn avatar_digest_b64(ciphertext: &[u8]) -> String {
    use base64::prelude::*;
    use sha2::{Digest as _, Sha256};
    let d = Sha256::digest(ciphertext);
    BASE64_STANDARD.encode(d)
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
        let plaintext = ProfilePlaintext { display_name: "Alice".into(), ..Default::default() };
        let blob = encrypt_profile(&plaintext, &key).unwrap();
        let decrypted = decrypt_profile(&blob, &key).unwrap();
        assert_eq!(decrypted.display_name, "Alice");
    }

    #[test]
    fn wrong_key_fails() {
        let key = generate_profile_key();
        let other = generate_profile_key();
        let plaintext = ProfilePlaintext { display_name: "Alice".into(), ..Default::default() };
        let blob = encrypt_profile(&plaintext, &key).unwrap();
        assert!(decrypt_profile(&blob, &other).is_err());
    }

    #[test]
    fn avatar_fields_round_trip() {
        let key = generate_profile_key();
        let plaintext = ProfilePlaintext {
            display_name: "Alice".into(),
            avatar_version: Some(3),
            avatar_digest: Some("YWJj".into()),
        };
        let blob = encrypt_profile(&plaintext, &key).unwrap();
        let out = decrypt_profile(&blob, &key).unwrap();
        assert_eq!(out.display_name, "Alice");
        assert_eq!(out.avatar_version, Some(3));
        assert_eq!(out.avatar_digest.as_deref(), Some("YWJj"));
    }

    #[test]
    fn old_format_without_avatar_still_decodes() {
        // A blob written by an older client carries only `display_name`. It must
        // still decode, with the avatar fields defaulting to absent.
        let key = generate_profile_key();
        let json = br#"{"display_name":"Bob"}"#;
        let cipher = Aes256Gcm::new((&key).into());
        let nonce_bytes: [u8; NONCE_LEN] = [7u8; NONCE_LEN];
        let ct = cipher.encrypt(Nonce::from_slice(&nonce_bytes), json.as_slice()).unwrap();
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ct);
        let out = decrypt_profile(&blob, &key).unwrap();
        assert_eq!(out.display_name, "Bob");
        assert_eq!(out.avatar_version, None);
        assert_eq!(out.avatar_digest, None);
    }

    #[test]
    fn no_avatar_serializes_without_avatar_keys() {
        // skip_serializing_if keeps the wire identical to the old format when no
        // avatar is set, so older clients see exactly what they saw before.
        let json = serde_json::to_string(&ProfilePlaintext {
            display_name: "Al".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(json, r#"{"display_name":"Al"}"#);
    }
}
