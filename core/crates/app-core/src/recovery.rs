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
pub fn build_recovery_plaintext(
    rotation_key_private: &[u8],
    identity_keypair_bytes: &[u8],
    servers: &[String],
) -> RecoveryBlobPlaintext {
    RecoveryBlobPlaintext {
        rotation_key: BASE64_STANDARD.encode(rotation_key_private),
        identity_keypair: BASE64_STANDARD.encode(identity_keypair_bytes),
        servers: servers.to_vec(),
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
        };

        let blob = encrypt_recovery_blob(&plaintext, &key).unwrap();
        let decrypted = decrypt_recovery_blob(&blob, &key).unwrap();

        assert_eq!(decrypted.rotation_key, plaintext.rotation_key);
        assert_eq!(decrypted.identity_keypair, plaintext.identity_keypair);
        assert_eq!(decrypted.servers, plaintext.servers);
    }

    #[test]
    fn wrong_key_fails() {
        let key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let plaintext = RecoveryBlobPlaintext {
            rotation_key: "dGVzdA==".into(),
            identity_keypair: "dGVzdA==".into(),
            servers: vec![],
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
