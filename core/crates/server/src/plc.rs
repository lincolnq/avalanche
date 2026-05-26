//! PLC directory client helpers shared across routes.
//!
//! The server fetches PLC state during registration (to verify that the
//! identity key submitted by the client matches the `#avalanche`
//! verification method in the DID document) and during device replacement
//! (to verify that the submitted rotation key is in the DID's
//! `rotationKeys` list — i.e. is actually authorized to rotate the DID).
//!
//! We hit `https://plc.directory/{did}/data` for the current state rather
//! than `/{did}` (the resolved DID document) because rotation keys are
//! PLC-specific metadata and are not part of the W3C DID document shape.

use serde::Deserialize;

use crate::error::ServerError;

const PLC_DIRECTORY_URL: &str = "https://plc.directory";

/// Multicodec varint prefix for an Ed25519 public key: `0xed 0x01`.
const MULTICODEC_ED25519: [u8; 2] = [0xed, 0x01];

/// Multicodec varint prefix for a P-256 public key: `0x80 0x24` (varint 0x1200).
const MULTICODEC_P256: [u8; 2] = [0x80, 0x24];

/// Current PLC state for a DID, as returned by `GET /{did}/data`.
///
/// Only the fields we need are deserialized; the rest of the response
/// (verification methods, services, alsoKnownAs) is ignored here.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlcData {
    #[serde(default)]
    pub rotation_keys: Vec<String>,
}

/// Fetch the current PLC state for a DID.
///
/// Returns `BadRequest` for unknown DIDs (4xx from the directory) and
/// `Internal` for transport or 5xx failures.
pub async fn fetch_plc_data(did: &str) -> Result<PlcData, ServerError> {
    let url = format!("{PLC_DIRECTORY_URL}/{did}/data");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| ServerError::Internal(format!("PLC directory request failed: {e}")))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ServerError::BadRequest(format!(
            "DID not found in PLC directory: {did}"
        )));
    }
    if !status.is_success() {
        return Err(ServerError::Internal(format!(
            "PLC directory returned {status}"
        )));
    }
    resp.json::<PlcData>()
        .await
        .map_err(|e| ServerError::Internal(format!("PLC directory parse failed: {e}")))
}

/// Fetch the P-256 rotation keys currently authorized for a DID.
///
/// Each entry is returned as compressed SEC1 bytes (33 bytes, 0x02/0x03
/// prefix + 32 bytes). Non-P-256 entries in the PLC `rotationKeys` list
/// are silently skipped — a PLC DID may list keys of multiple curves;
/// we only honor the ones the server can verify.
pub async fn fetch_rotation_keys_p256(did: &str) -> Result<Vec<Vec<u8>>, ServerError> {
    let data = fetch_plc_data(did).await?;
    Ok(data
        .rotation_keys
        .iter()
        .filter_map(|k| decode_did_key_p256(k).ok())
        .collect())
}

/// Decode a `did:key:z...` to raw bytes for a specific multicodec prefix.
fn decode_did_key(did_key: &str, expected_prefix: [u8; 2]) -> Result<Vec<u8>, String> {
    let z_part = did_key
        .strip_prefix("did:key:z")
        .ok_or("did:key must start with did:key:z")?;
    let bytes = bs58::decode(z_part)
        .into_vec()
        .map_err(|e| format!("base58 decode failed: {e}"))?;
    if bytes.len() < 2 {
        return Err("did:key payload too short".into());
    }
    if bytes[0] != expected_prefix[0] || bytes[1] != expected_prefix[1] {
        return Err(format!(
            "unexpected multicodec prefix [{:#04x},{:#04x}]",
            bytes[0], bytes[1]
        ));
    }
    Ok(bytes[2..].to_vec())
}

/// Decode a `did:key:z...` (Ed25519) to the raw 32-byte public key.
pub fn decode_did_key_ed25519(did_key: &str) -> Result<Vec<u8>, String> {
    decode_did_key(did_key, MULTICODEC_ED25519)
}

/// Decode a `did:key:z...` (P-256) to the compressed SEC1 public key
/// (33 bytes: 0x02/0x03 prefix + 32 bytes).
pub fn decode_did_key_p256(did_key: &str) -> Result<Vec<u8>, String> {
    decode_did_key(did_key, MULTICODEC_P256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_curve_prefix() {
        // An Ed25519-prefixed did:key should fail when decoded as P-256.
        let raw = [0x42u8; 32];
        let mut bytes = MULTICODEC_ED25519.to_vec();
        bytes.extend_from_slice(&raw);
        let did_key = format!("did:key:z{}", bs58::encode(&bytes).into_string());
        assert!(decode_did_key_p256(&did_key).is_err());
        assert_eq!(decode_did_key_ed25519(&did_key).unwrap(), raw);
    }

    #[test]
    fn p256_round_trip() {
        // Build a synthetic compressed P-256 pubkey (33 bytes, 0x02 prefix).
        let mut raw = vec![0x02u8];
        raw.extend_from_slice(&[0x11u8; 32]);
        let mut bytes = MULTICODEC_P256.to_vec();
        bytes.extend_from_slice(&raw);
        let did_key = format!("did:key:z{}", bs58::encode(&bytes).into_string());
        assert_eq!(decode_did_key_p256(&did_key).unwrap(), raw);
    }
}
