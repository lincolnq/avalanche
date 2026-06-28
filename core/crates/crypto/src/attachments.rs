//! Attachment blob encryption (docs/35-attachments.md).
//!
//! Attachments are encrypted with **AES-256-CBC + HMAC-SHA-256**
//! (encrypt-then-MAC), exactly as Signal does for attachments — deliberately
//! *not* the AES-256-GCM the ratchet uses for messages, so large media can be
//! verified incrementally as it streams in. This module wraps libsignal's
//! audited primitives ([`signal_crypto::aes_256_cbc_encrypt`] /
//! [`signal_crypto::CryptographicMac`]); it implements no crypto itself and,
//! like the rest of this crate, performs no I/O.
//!
//! ## Stored blob layout
//!
//! ```text
//! IV(16) ‖ AES-256-CBC(PKCS7(bucket-padded plaintext)) ‖ HMAC-SHA-256(IV ‖ ct)
//! ```
//!
//! The 64-byte attachment key is `aes_key(32) ‖ hmac_key(32)`, generated fresh
//! per attachment ([`generate_key`]) and carried in the E2E message pointer.
//! The `digest` is `SHA-256` over the *entire* stored blob; the recipient
//! recomputes it before touching the bytes, so a malicious store cannot
//! substitute content.
//!
//! The plaintext is padded up to a geometric bucket size ([`padded_size`])
//! before encryption so the stored ciphertext length leaks only the bucket, not
//! the exact size. The unpadded length travels in the pointer (`size_bytes`)
//! and the recipient trims after decrypting.

use rand::{rngs::OsRng, RngCore, TryRngCore as _};
use sha2::{Digest, Sha256};
use signal_crypto::{aes_256_cbc_decrypt, aes_256_cbc_encrypt, CryptographicMac};
use thiserror::Error;

/// Length of the attachment key: AES-256 key (32) ‖ HMAC-SHA-256 key (32).
pub const KEY_LEN: usize = 64;
/// AES-CBC initialization vector length.
pub const IV_LEN: usize = 16;
/// HMAC-SHA-256 / SHA-256 output length.
pub const MAC_LEN: usize = 32;
/// SHA-256 digest length.
pub const DIGEST_LEN: usize = 32;

const AES_KEY_LEN: usize = 32;

/// Errors from attachment encrypt/decrypt.
#[derive(Debug, Error)]
pub enum AttachmentError {
    /// The 64-byte key was the wrong length.
    #[error("attachment key must be {KEY_LEN} bytes")]
    InvalidKey,
    /// The stored blob was shorter than `IV ‖ one AES block ‖ MAC`.
    #[error("attachment blob is malformed (too short)")]
    MalformedBlob,
    /// `SHA-256(blob)` did not match the pointer's `digest`. Aborts before any
    /// decryption is attempted.
    #[error("attachment digest mismatch")]
    DigestMismatch,
    /// The HMAC tag did not verify — the ciphertext was tampered with.
    #[error("attachment MAC verification failed")]
    MacMismatch,
    /// The declared unpadded length exceeds the recovered plaintext.
    #[error("attachment plaintext length out of range")]
    BadLength,
    /// libsignal's AES-CBC layer rejected the input.
    #[error("attachment cipher error: {0}")]
    Cipher(String),
}

/// An encrypted attachment ready to upload, plus the digest to put in the
/// pointer.
#[derive(Clone, Debug)]
pub struct EncryptedAttachment {
    /// `IV ‖ ciphertext ‖ MAC` — the exact bytes uploaded to the blob store.
    pub blob: Vec<u8>,
    /// `SHA-256(blob)` — bound into the E2E pointer for tamper detection.
    pub digest: Vec<u8>,
}

/// Generate a fresh 64-byte attachment key (`aes_key(32) ‖ hmac_key(32)`).
///
/// Never reused, never derived from the ratchet — one key per attachment.
pub fn generate_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    OsRng.unwrap_err().fill_bytes(&mut key);
    key
}

/// The bucket size `plaintext` is padded up to before encryption.
///
/// Signal's monotonic ~5% geometric bucketing: `max(541, floor(1.05^⌈log₁.₀₅
/// n⌉))`, clamped so the result is never smaller than the input. The 541-byte
/// floor matches Signal so small files don't reveal their size.
pub fn padded_size(unpadded: usize) -> usize {
    const MIN_BUCKET: usize = 541;
    if unpadded <= MIN_BUCKET {
        return MIN_BUCKET;
    }
    let n = unpadded as f64;
    let exponent = (n.ln() / 1.05_f64.ln()).ceil();
    let bucket = 1.05_f64.powf(exponent).floor() as usize;
    // Guard against floating-point rounding ever producing a bucket below the
    // input — padding must never truncate.
    bucket.max(unpadded)
}

/// Pad + encrypt-then-MAC `plaintext` under the 64-byte `key`.
///
/// A fresh random IV is generated and prepended; the caller only manages the
/// key. Returns the uploadable blob and its digest.
pub fn encrypt(plaintext: &[u8], key: &[u8]) -> Result<EncryptedAttachment, AttachmentError> {
    let (aes_key, hmac_key) = split_key(key)?;

    // Pad up to the bucket with zero bytes; the true length rides in the
    // pointer's `size_bytes` and is restored on decrypt.
    let target = padded_size(plaintext.len());
    let mut padded = Vec::with_capacity(target);
    padded.extend_from_slice(plaintext);
    padded.resize(target, 0);

    let mut iv = [0u8; IV_LEN];
    OsRng.unwrap_err().fill_bytes(&mut iv);

    let ciphertext =
        aes_256_cbc_encrypt(&padded, aes_key, &iv).map_err(|e| AttachmentError::Cipher(e.to_string()))?;

    let mut mac = CryptographicMac::new("HmacSha256", hmac_key)
        .map_err(|e| AttachmentError::Cipher(e.to_string()))?;
    mac.update(&iv);
    mac.update(&ciphertext);
    let tag = mac.finalize();

    let mut blob = Vec::with_capacity(IV_LEN + ciphertext.len() + MAC_LEN);
    blob.extend_from_slice(&iv);
    blob.extend_from_slice(&ciphertext);
    blob.extend_from_slice(&tag);

    let digest = Sha256::digest(&blob).to_vec();
    Ok(EncryptedAttachment { blob, digest })
}

/// Verify `digest`, verify the HMAC, decrypt, and trim to `plaintext_len`.
///
/// The digest is checked **first**, before any decryption, so a substituted
/// blob is rejected without running the cipher. `plaintext_len` is the
/// pointer's unpadded `size_bytes`.
pub fn decrypt(
    blob: &[u8],
    key: &[u8],
    digest: &[u8],
    plaintext_len: usize,
) -> Result<Vec<u8>, AttachmentError> {
    let (aes_key, hmac_key) = split_key(key)?;

    // Smallest valid blob: IV + one AES block + MAC.
    if blob.len() < IV_LEN + 16 + MAC_LEN {
        return Err(AttachmentError::MalformedBlob);
    }

    // 1. Digest over the exact downloaded bytes — abort before decrypting.
    let actual_digest = Sha256::digest(blob);
    if !constant_time_eq(actual_digest.as_slice(), digest) {
        return Err(AttachmentError::DigestMismatch);
    }

    let (iv, rest) = blob.split_at(IV_LEN);
    let (ciphertext, tag) = rest.split_at(rest.len() - MAC_LEN);

    // 2. HMAC over IV ‖ ciphertext.
    let mut mac = CryptographicMac::new("HmacSha256", hmac_key)
        .map_err(|e| AttachmentError::Cipher(e.to_string()))?;
    mac.update(iv);
    mac.update(ciphertext);
    if !constant_time_eq(&mac.finalize(), tag) {
        return Err(AttachmentError::MacMismatch);
    }

    // 3. Decrypt, then trim the bucket padding back to the true length.
    let padded = aes_256_cbc_decrypt(ciphertext, aes_key, iv)
        .map_err(|e| AttachmentError::Cipher(e.to_string()))?;
    if plaintext_len > padded.len() {
        return Err(AttachmentError::BadLength);
    }
    Ok(padded[..plaintext_len].to_vec())
}

fn split_key(key: &[u8]) -> Result<(&[u8], &[u8]), AttachmentError> {
    if key.len() != KEY_LEN {
        return Err(AttachmentError::InvalidKey);
    }
    Ok(key.split_at(AES_KEY_LEN))
}

/// Constant-time byte comparison — avoids leaking MAC/digest match position
/// through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = generate_key();
        let plaintext = b"the quick brown fox jumps over the lazy dog".to_vec();
        let enc = encrypt(&plaintext, &key).unwrap();
        let dec = decrypt(&enc.blob, &key, &enc.digest, plaintext.len()).unwrap();
        assert_eq!(dec, plaintext);
    }

    #[test]
    fn round_trip_empty() {
        let key = generate_key();
        let enc = encrypt(b"", &key).unwrap();
        // Empty plaintext still pads to the minimum bucket and round-trips.
        let dec = decrypt(&enc.blob, &key, &enc.digest, 0).unwrap();
        assert_eq!(dec, b"");
    }

    #[test]
    fn digest_mismatch_rejected_before_decrypt() {
        let key = generate_key();
        let enc = encrypt(b"secret", &key).unwrap();
        let mut bad_digest = enc.digest.clone();
        bad_digest[0] ^= 0xff;
        assert!(matches!(
            decrypt(&enc.blob, &key, &bad_digest, 6),
            Err(AttachmentError::DigestMismatch)
        ));
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let key = generate_key();
        let enc = encrypt(b"secret", &key).unwrap();
        let mut blob = enc.blob.clone();
        // Flip a byte in the ciphertext region (after the IV).
        blob[IV_LEN + 1] ^= 0x01;
        // Digest is recomputed over the tampered blob, so it fails the digest
        // check first (digest binds the exact bytes). Recompute the digest to
        // exercise the MAC path specifically.
        let digest = Sha256::digest(&blob).to_vec();
        assert!(matches!(
            decrypt(&blob, &key, &digest, 6),
            Err(AttachmentError::MacMismatch)
        ));
    }

    #[test]
    fn wrong_key_length_rejected() {
        assert!(matches!(
            encrypt(b"x", &[0u8; 32]),
            Err(AttachmentError::InvalidKey)
        ));
    }

    #[test]
    fn padded_size_is_never_smaller_and_monotonic() {
        assert_eq!(padded_size(0), 541);
        assert_eq!(padded_size(541), 541);
        let mut prev = 0;
        for n in [542usize, 1000, 1001, 100_000, 5_000_000, 100_000_000] {
            let p = padded_size(n);
            assert!(p >= n, "bucket {p} for {n} smaller than input");
            assert!(p >= prev, "padded_size not monotonic at {n}");
            prev = p;
        }
        // Bucketing is coarse: many nearby sizes collapse to far fewer buckets.
        let buckets: std::collections::HashSet<usize> =
            (1000..1100).map(padded_size).collect();
        assert!(buckets.len() < 20, "buckets too fine: {}", buckets.len());
    }

    proptest::proptest! {
        #[test]
        fn round_trip_arbitrary_bytes(plaintext: Vec<u8>) {
            let key = generate_key();
            let enc = encrypt(&plaintext, &key).unwrap();
            let dec = decrypt(&enc.blob, &key, &enc.digest, plaintext.len()).unwrap();
            proptest::prop_assert_eq!(dec, plaintext);
        }

        #[test]
        fn padded_size_never_truncates(n in 0usize..200_000_000) {
            proptest::prop_assert!(padded_size(n) >= n);
        }
    }
}
