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

/// Build an unsigned genesis operation **with no identity key**.
///
/// The genesis op intentionally omits `verification_methods` so the resulting
/// DID is a function of only the rotation key + signup server URL. This is
/// what makes the DID deterministically recoverable from the passkey alone
/// (see `50-identity-auth-recovery.md`).
///
/// The identity key is added immediately afterward via
/// [`build_identity_update_op`].
///
/// - `rotation_key_pub`: compressed P-256 public key bytes
/// - `server_url`: the homeserver URL to list as the service endpoint
pub fn build_genesis_op(rotation_key_pub: &[u8], server_url: &str) -> PlcOperation {
    let rotation_did_key = did_key_p256(rotation_key_pub);

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
        verification_methods: BTreeMap::new(),
        also_known_as: vec![],
        services,
        prev: None,
        sig: None,
    }
}

/// Build an unsigned PLC update operation that adds the device identity key
/// as a verification method.
///
/// Submitted immediately after the genesis op at signup. The rotation keys
/// and services carry over unchanged from the genesis state; only the
/// `verification_methods` map gains the `avalanche` entry.
///
/// - `rotation_key_pub`: must match the genesis rotation key
/// - `identity_key_pub`: Ed25519 public key bytes (32 bytes, from libsignal)
/// - `server_url`: must match the genesis service endpoint
/// - `prev_cid`: the CIDv1 of the signed genesis operation (see
///   [`plc_op_cid`])
pub fn build_identity_update_op(
    rotation_key_pub: &[u8],
    identity_key_pub: &[u8],
    server_url: &str,
    prev_cid: &str,
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
        prev: Some(prev_cid.to_string()),
        sig: None,
    }
}

/// Compute the CID for a signed PLC operation, used as the `prev` reference
/// in the next op in the chain. Per PLC spec this is a CIDv1 with dag-cbor
/// codec (0x71) and sha2-256 multihash (0x12), multibase-encoded as base32
/// lowercase no-pad with the 'b' prefix — e.g. `bafyrei...`. Note this is
/// *not* the same encoding used to derive the DID itself, which strips the
/// CID framing and truncates the hash.
pub fn plc_op_cid(signed_op: &PlcOperation) -> Result<String, AppError> {
    let cbor_bytes = serde_ipld_dagcbor::to_vec(signed_op)
        .map_err(|e| AppError::Protocol(format!("DAG-CBOR encode failed: {e}")))?;
    let hash = Sha256::digest(&cbor_bytes);
    // CIDv1 = [version=0x01, codec=0x71 (dag-cbor), multihash=0x12 0x20 (sha2-256, 32 bytes), ...hash]
    let mut cid_bytes = Vec::with_capacity(4 + hash.len());
    cid_bytes.extend_from_slice(&[0x01, 0x71, 0x12, 0x20]);
    cid_bytes.extend_from_slice(&hash);
    let encoded = base32::encode(
        base32::Alphabet::Rfc4648Lower { padding: false },
        &cid_bytes,
    );
    Ok(format!("b{encoded}"))
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

/// Submit a signed PLC operation (genesis or update) to the PLC directory.
///
/// POST `https://plc.directory/{did}` with the signed operation as JSON body.
/// Always POSTs — no idempotency check; for that, use [`ensure_op_submitted`].
pub async fn submit_op(did: &str, signed_op: &PlcOperation) -> Result<(), AppError> {
    submit_op_inner(&reqwest::Client::new(), did, signed_op).await
}

/// Outcome of [`ensure_op_submitted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// Op was POSTed and accepted.
    Submitted,
    /// Op was already on the chain at `expected_index` byte-for-byte; no POST made.
    AlreadyApplied,
}

/// Submit `signed_op` idempotently: if the DID's existing op chain at
/// `expected_index` already equals `signed_op` (CBOR-identical), do nothing
/// and return [`SubmitOutcome::AlreadyApplied`]. Otherwise POST it.
///
/// `expected_index` is the 0-based position this op is expected to occupy
/// in the op log: 0 for genesis, 1 for the first update, etc.
///
/// This makes signup resumable after a partial failure: the rotation key is
/// deterministic from the passkey PRF + signup server URL, so a byte-match
/// against the directory's existing op proves we already submitted it and
/// it's safe to skip.
pub async fn ensure_op_submitted(
    did: &str,
    signed_op: &PlcOperation,
    expected_index: usize,
) -> Result<SubmitOutcome, AppError> {
    let client = reqwest::Client::new();

    if let Some(existing) = fetch_log(&client, did).await {
        if let Some(at_index) = existing.get(expected_index) {
            // Something is already at the slot we want to occupy.
            if op_cbor_equals(at_index, signed_op)? {
                // Byte-identical — we previously submitted this exact op. Skip.
                return Ok(SubmitOutcome::AlreadyApplied);
            }
            // Different content. The chain has moved past where we tried to
            // insert and we can't overwrite. This is the "passkey already
            // owns a fully-registered identity, but local store is empty"
            // case (e.g. reinstall after a successful signup).
            return Err(AppError::Protocol(format!(
                "DID {did} already has op at index {expected_index} that differs from \
                 the one we're trying to submit; this passkey is already registered \
                 (chain length {len}). Use recovery to restore the identity instead \
                 of registering again.",
                len = existing.len(),
            )));
        }
        // Chain exists but is shorter than expected_index — we're extending it.
        // Defensive sanity-check: refuse to write into a gap.
        if existing.len() < expected_index {
            return Err(AppError::Protocol(format!(
                "DID {did} chain length is {len}, can't submit op at index {expected_index} \
                 (would leave a gap)",
                len = existing.len(),
            )));
        }
    }

    submit_op_inner(&client, did, signed_op).await?;
    Ok(SubmitOutcome::Submitted)
}

/// Compare two ops by their canonical DAG-CBOR encoding.
fn op_cbor_equals(a: &PlcOperation, b: &PlcOperation) -> Result<bool, AppError> {
    let ab = serde_ipld_dagcbor::to_vec(a)
        .map_err(|e| AppError::Protocol(format!("DAG-CBOR encode failed: {e}")))?;
    let bb = serde_ipld_dagcbor::to_vec(b)
        .map_err(|e| AppError::Protocol(format!("DAG-CBOR encode failed: {e}")))?;
    Ok(ab == bb)
}

async fn fetch_log(client: &reqwest::Client, did: &str) -> Option<Vec<PlcOperation>> {
    let url = format!("{PLC_DIRECTORY_URL}/{did}/log");
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
}

async fn submit_op_inner(
    client: &reqwest::Client,
    did: &str,
    signed_op: &PlcOperation,
) -> Result<(), AppError> {
    let url = format!("{PLC_DIRECTORY_URL}/{did}");
    let resp = client
        .post(&url)
        .json(signed_op)
        .send()
        .await
        .map_err(|e| AppError::Protocol(format!("PLC directory request failed: {e}")))?;

    if resp.status().is_success() {
        return Ok(());
    }

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let prev_summary = signed_op.prev.as_deref().unwrap_or("null");
    let existing = describe_existing(client, did).await;
    Err(AppError::Protocol(format!(
        "PLC directory rejected op: {status} — {body} \
         (did={did}, op.prev={prev_summary}, {existing})"
    )))
}

/// Best-effort probe of plc.directory to describe whether `did` already
/// exists and, if so, what its current tip CID is. Used only for error
/// messages — any failure here is swallowed into a short string.
async fn describe_existing(client: &reqwest::Client, did: &str) -> String {
    let log_url = format!("{PLC_DIRECTORY_URL}/{did}/log");
    let resp = match client.get(&log_url).send().await {
        Ok(r) => r,
        Err(e) => return format!("probe failed: {e}"),
    };
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return "did not yet registered".into();
    }
    if !resp.status().is_success() {
        return format!("probe status: {}", resp.status());
    }
    // The /log endpoint returns an array of signed ops; the last one's CID
    // is the tip that a new op must chain from.
    let ops: Vec<PlcOperation> = match resp.json().await {
        Ok(v) => v,
        Err(e) => return format!("probe parse failed: {e}"),
    };
    let tip = match ops.last() {
        None => return "did exists but log is empty".into(),
        Some(op) => op,
    };
    match plc_op_cid(tip) {
        Ok(cid) => format!("did already exists, op_count={}, tip_cid={cid}", ops.len()),
        Err(e) => format!("did already exists, op_count={}, tip cid err: {e}", ops.len()),
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

/// Extract the AvalancheHomeserver service endpoint from a resolved DID
/// document. The resolved document has `service` as an array of objects with
/// `id`, `type`, and `serviceEndpoint` fields (not the BTreeMap used in PLC
/// operations).
pub fn extract_homeserver_endpoint(doc: &serde_json::Value) -> Option<String> {
    doc.get("service")?
        .as_array()?
        .iter()
        .find(|s| {
            s.get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t == "AvalancheHomeserver")
        })
        .and_then(|s| s.get("serviceEndpoint").and_then(|e| e.as_str()))
        .map(|s| s.to_string())
}

/// Look up a `did:plc:*` in the PLC directory and return the homeserver URL
/// advertised in its DID document.
pub async fn resolve_homeserver_url(did: &str) -> Result<String, AppError> {
    let doc = resolve_did(did).await?;
    extract_homeserver_endpoint(&doc).ok_or_else(|| {
        AppError::Protocol(format!(
            "DID document for {did} has no AvalancheHomeserver service"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::generate_rotation_key;

    #[test]
    fn extract_homeserver_endpoint_finds_avalanche_service() {
        let doc = serde_json::json!({
            "service": [
                { "id": "#other", "type": "OtherService", "serviceEndpoint": "https://nope" },
                { "id": "#hs", "type": "AvalancheHomeserver", "serviceEndpoint": "https://hs.example" },
            ]
        });
        assert_eq!(
            extract_homeserver_endpoint(&doc).as_deref(),
            Some("https://hs.example")
        );
    }

    #[test]
    fn extract_homeserver_endpoint_missing_returns_none() {
        let doc = serde_json::json!({ "service": [] });
        assert_eq!(extract_homeserver_endpoint(&doc), None);

        let doc = serde_json::json!({});
        assert_eq!(extract_homeserver_endpoint(&doc), None);
    }

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

        let op = build_genesis_op(&pub_key, "https://example.com");
        assert_eq!(op.r#type, "plc_operation");
        assert!(op.prev.is_none());
        assert!(op.sig.is_none());
        assert_eq!(op.rotation_keys.len(), 1);
        // Genesis intentionally has no identity key — added via update op.
        assert!(op.verification_methods.is_empty());

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

        let (priv_key, pub_key) = generate_rotation_key();
        let op = build_genesis_op(&pub_key, "https://example.com");

        let signed = sign_plc_op(&op, &priv_key).unwrap();
        let sig_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(signed.sig.as_ref().unwrap())
            .unwrap();
        let signature = Signature::from_bytes((&sig_bytes[..]).into()).unwrap();

        let mut unsigned = op.clone();
        unsigned.sig = None;
        let cbor_bytes = serde_ipld_dagcbor::to_vec(&unsigned).unwrap();

        let signing_key = SigningKey::from_bytes((&priv_key[..]).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        verifying_key.verify(&cbor_bytes, &signature).unwrap();
    }

    #[test]
    fn identity_update_op_chains_to_genesis() {
        let (priv_key, pub_key) = generate_rotation_key();
        let identity_key = [9u8; 32];
        let server = "https://example.com";

        let genesis = build_genesis_op(&pub_key, server);
        let signed_genesis = sign_plc_op(&genesis, &priv_key).unwrap();
        let did = derive_did(&signed_genesis).unwrap();
        let cid = plc_op_cid(&signed_genesis).unwrap();

        let update = build_identity_update_op(&pub_key, &identity_key, server, &cid);
        assert_eq!(update.prev.as_deref(), Some(cid.as_str()));
        // CIDv1 dag-cbor + sha2-256 always encodes to a `bafyrei…` string.
        assert!(cid.starts_with("bafyrei"), "expected CIDv1 prefix, got {cid}");
        assert_eq!(cid.len(), 59);
        assert_eq!(update.verification_methods.len(), 1);
        // Rotation keys and services carry over unchanged.
        assert_eq!(update.rotation_keys, signed_genesis.rotation_keys);
        assert_eq!(update.services.len(), 1);

        let signed_update = sign_plc_op(&update, &priv_key).unwrap();
        assert!(signed_update.sig.is_some());

        // DID is fixed by genesis, unaffected by subsequent ops.
        let did_after = derive_did(&signed_genesis).unwrap();
        assert_eq!(did, did_after);
    }

    #[test]
    fn did_is_deterministic_from_rotation_and_server() {
        // Same rotation key + server URL → same DID. Load-bearing for the
        // "recover the DID from passkey + signup server URL alone" property
        // documented in 50-identity-auth-recovery.md. Relies on RFC 6979
        // deterministic ECDSA, which is the p256 crate's default for `sign`.
        let (priv_key, pub_key) = generate_rotation_key();
        let server = "https://example.com";

        let did1 = derive_did(
            &sign_plc_op(&build_genesis_op(&pub_key, server), &priv_key).unwrap(),
        )
        .unwrap();
        let did2 = derive_did(
            &sign_plc_op(&build_genesis_op(&pub_key, server), &priv_key).unwrap(),
        )
        .unwrap();
        assert_eq!(did1, did2, "DID must be deterministic for recovery");
    }
}
