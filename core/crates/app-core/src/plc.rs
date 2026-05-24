//! DID PLC directory operations: genesis, signing, submission.
//!
//! Implements the `did:plc` method spec (v0.1):
//! - Build a genesis operation with rotation keys, verification methods, services
//! - Sign with P-256 rotation key (ECDSA-SHA256, low-S, raw r||s, base64url)
//! - Derive the DID from SHA-256 of DAG-CBOR encoded signed operation
//! - Submit to the PLC directory via HTTP POST

use base64::prelude::*;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::error::AppError;

/// The public PLC directory endpoint.
const PLC_DIRECTORY_URL: &str = "https://plc.directory";

/// Multicodec prefix for P-256 public key (compressed): varint 0x1200 = [0x80, 0x24]
const MULTICODEC_P256: &[u8] = &[0x80, 0x24];

/// Multicodec prefix for Ed25519 public key: varint 0xed = [0xed, 0x01]
const MULTICODEC_ED25519: &[u8] = &[0xed, 0x01];

/// Encode a P-256 compressed public key as a `did:key` string.
pub fn did_key_p256(compressed_pub: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(MULTICODEC_P256.len() + compressed_pub.len());
    bytes.extend_from_slice(MULTICODEC_P256);
    bytes.extend_from_slice(compressed_pub);
    format!("did:key:z{}", bs58::encode(&bytes).into_string())
}

/// Encode an Ed25519 public key as a `did:key` string.
///
/// Accepts either 32 raw bytes or 33 bytes (libsignal format with 0x05 prefix).
/// The `did:key` always uses the raw 32-byte key.
pub fn did_key_ed25519(pub_key: &[u8]) -> String {
    // Strip libsignal's 0x05 type prefix if present.
    let raw = if pub_key.len() == 33 && pub_key[0] == 0x05 {
        &pub_key[1..]
    } else {
        pub_key
    };
    let mut bytes = Vec::with_capacity(MULTICODEC_ED25519.len() + raw.len());
    bytes.extend_from_slice(MULTICODEC_ED25519);
    bytes.extend_from_slice(raw);
    format!("did:key:z{}", bs58::encode(&bytes).into_string())
}

/// A PLC operation (genesis or update).
///
/// Field order matters for DAG-CBOR deterministic encoding — we use BTreeMap
/// for services and verificationMethods to ensure sorted keys.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlcOperation {
    pub r#type: String,
    pub rotation_keys: Vec<String>,
    pub verification_methods: BTreeMap<String, String>,
    pub also_known_as: Vec<String>,
    pub services: BTreeMap<String, PlcService>,
    pub prev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlcService {
    pub r#type: String,
    pub endpoint: String,
}

/// Build an unsigned genesis operation.
///
/// - `rotation_key_pub`: compressed P-256 public key bytes
/// - `identity_key_pub`: Ed25519 public key bytes (32 bytes, from libsignal)
/// - `server_url`: the homeserver URL to list as the service endpoint
pub fn build_genesis_op(
    rotation_key_pub: &[u8],
    identity_key_pub: &[u8],
    server_url: &str,
) -> PlcOperation {
    let rotation_did_key = did_key_p256(rotation_key_pub);
    let identity_did_key = did_key_ed25519(identity_key_pub);

    let mut verification_methods = BTreeMap::new();
    verification_methods.insert("avalanche".to_string(), identity_did_key);

    let mut services = BTreeMap::new();
    services.insert(
        "avalanche_homeserver".to_string(),
        PlcService {
            r#type: "AvalancheHomeserver".to_string(),
            endpoint: server_url.to_string(),
        },
    );

    PlcOperation {
        r#type: "plc_operation".to_string(),
        rotation_keys: vec![rotation_did_key],
        verification_methods,
        also_known_as: vec![],
        services,
        prev: None,
        sig: None,
    }
}

/// Sign a PLC operation with the rotation key.
///
/// The signing process per the spec:
/// 1. Remove the `sig` field
/// 2. Encode as DAG-CBOR
/// 3. Sign with ECDSA-SHA256
/// 4. Normalize to low-S
/// 5. Encode raw (r || s) as base64url no-pad
pub fn sign_plc_op(
    op: &PlcOperation,
    rotation_key_private: &[u8],
) -> Result<PlcOperation, AppError> {
    // 1. Build the unsigned version (sig = None)
    let mut unsigned = op.clone();
    unsigned.sig = None;

    // 2. Encode as DAG-CBOR
    let cbor_bytes = serde_ipld_dagcbor::to_vec(&unsigned)
        .map_err(|e| AppError::Protocol(format!("DAG-CBOR encode failed: {e}")))?;

    // 3. Sign with P-256
    let signing_key = SigningKey::from_bytes(rotation_key_private.into())
        .map_err(|e| AppError::Protocol(format!("invalid rotation key: {e}")))?;
    let sig: Signature = signing_key.sign(&cbor_bytes);

    // 4. Normalize to low-S (p256 crate does this by default)
    let normalized = sig.normalize_s().unwrap_or(sig);

    // 5. Encode raw r||s (64 bytes) as base64url no-pad
    let sig_bytes = normalized.to_bytes();
    let sig_b64 = BASE64_URL_SAFE_NO_PAD.encode(sig_bytes);

    let mut signed = op.clone();
    signed.sig = Some(sig_b64);
    Ok(signed)
}

/// Derive the `did:plc` identifier from a signed genesis operation.
///
/// Process: DAG-CBOR encode the signed op → SHA-256 → base32 lowercase → first 24 chars.
pub fn derive_did(signed_op: &PlcOperation) -> Result<String, AppError> {
    let cbor_bytes = serde_ipld_dagcbor::to_vec(signed_op)
        .map_err(|e| AppError::Protocol(format!("DAG-CBOR encode failed: {e}")))?;

    let hash = Sha256::digest(&cbor_bytes);
    let encoded = base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &hash);
    Ok(format!("did:plc:{}", &encoded[..24]))
}

/// Submit a signed genesis operation to the PLC directory.
///
/// POST `https://plc.directory/{did}` with the signed operation as JSON body.
pub async fn submit_genesis(did: &str, signed_op: &PlcOperation) -> Result<(), AppError> {
    let url = format!("{PLC_DIRECTORY_URL}/{did}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(signed_op)
        .send()
        .await
        .map_err(|e| AppError::Protocol(format!("PLC directory request failed: {e}")))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Protocol(format!(
            "PLC directory rejected genesis op: {status} — {body}"
        )))
    }
}

/// Resolve a DID from the PLC directory. Returns the DID document as JSON.
pub async fn resolve_did(did: &str) -> Result<serde_json::Value, AppError> {
    let url = format!("{PLC_DIRECTORY_URL}/{did}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Protocol(format!("PLC directory request failed: {e}")))?;

    if resp.status().is_success() {
        resp.json()
            .await
            .map_err(|e| AppError::Protocol(format!("PLC directory response parse failed: {e}")))
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Protocol(format!(
            "PLC directory lookup failed: {status} — {body}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::generate_rotation_key;

    #[test]
    fn did_key_p256_encoding() {
        // A known P-256 compressed public key (33 bytes: 0x02 or 0x03 prefix + 32 bytes)
        let (_, pub_key) = generate_rotation_key();
        let did_key = did_key_p256(&pub_key);
        assert!(did_key.starts_with("did:key:z"));
        // P-256 compressed = 33 bytes + 2 byte prefix = 35 bytes → base58 should be ~48 chars
        assert!(did_key.len() > 50);
    }

    #[test]
    fn did_key_ed25519_encoding() {
        // 32 bytes of fake Ed25519 key
        let pub_key = [42u8; 32];
        let did_key = did_key_ed25519(&pub_key);
        assert!(did_key.starts_with("did:key:z"));
    }

    #[test]
    fn genesis_op_round_trip() {
        let (priv_key, pub_key) = generate_rotation_key();
        let identity_key = [1u8; 32]; // fake Ed25519 key

        let op = build_genesis_op(&pub_key, &identity_key, "https://example.com");
        assert_eq!(op.r#type, "plc_operation");
        assert!(op.prev.is_none());
        assert!(op.sig.is_none());
        assert_eq!(op.rotation_keys.len(), 1);

        let signed = sign_plc_op(&op, &priv_key).unwrap();
        assert!(signed.sig.is_some());

        let did = derive_did(&signed).unwrap();
        assert!(did.starts_with("did:plc:"));
        assert_eq!(did.len(), "did:plc:".len() + 24);

        // Deriving again should be deterministic
        let did2 = derive_did(&signed).unwrap();
        assert_eq!(did, did2);
    }

    #[test]
    fn signature_is_valid() {
        use p256::ecdsa::{signature::Verifier, VerifyingKey};

        let (priv_key, _pub_key) = generate_rotation_key();
        let identity_key = [1u8; 32];
        let op = build_genesis_op(&_pub_key, &identity_key, "https://example.com");

        let signed = sign_plc_op(&op, &priv_key).unwrap();
        let sig_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(signed.sig.as_ref().unwrap())
            .unwrap();
        let signature = Signature::from_bytes((&sig_bytes[..]).into()).unwrap();

        // Verify against unsigned DAG-CBOR
        let mut unsigned = op.clone();
        unsigned.sig = None;
        let cbor_bytes = serde_ipld_dagcbor::to_vec(&unsigned).unwrap();

        let signing_key = SigningKey::from_bytes((&priv_key[..]).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        verifying_key.verify(&cbor_bytes, &signature).unwrap();
    }
}
