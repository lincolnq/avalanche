//! Request and response types for the homeserver API.
//!
//! These are the client-side representations. Key material and ciphertext are
//! raw bytes here; the `Client` methods handle base64 encoding/decoding for
//! the wire format.

use base64::prelude::*;
use serde::Deserialize;

use crate::error::NetError;

// ── Registration ─────────────────────────────────────────────────────────────

pub struct RegisterRequest {
    /// Client-generated DID (from PLC directory genesis operation).
    pub did: Option<String>,
    pub identity_key: Vec<u8>,
    pub registration_id: i32,
    pub device_id: i32,
    pub signed_prekey_id: i32,
    pub signed_prekey_public: Vec<u8>,
    pub signed_prekey_signature: Vec<u8>,
    pub one_time_prekeys: Vec<(i32, Vec<u8>)>,
    pub kyber_prekey_id: i32,
    pub kyber_prekey_public: Vec<u8>,
    pub kyber_prekey_signature: Vec<u8>,
    /// Plaintext display name — bot accounts only. Human accounts should pass `None`.
    pub display_name: Option<String>,
    pub is_bot: bool,
    /// Reserved suffix for the server-generated `did:local:` DID. Bot accounts
    /// only. When `Some("adminbot")` the server returns `did:local:adminbot`.
    pub did_suffix: Option<String>,
    /// Encrypted recovery blob (opaque ciphertext). Optional.
    pub recovery_blob: Option<Vec<u8>>,
    /// Encrypted profile blob (AES-256-GCM under the user's profile key). Optional.
    pub encrypted_profile: Option<Vec<u8>>,
    /// Ed25519 signature of `"register:{did}"` proving identity key possession.
    /// Required when `did` is provided.
    pub identity_key_signature: Option<String>,
    /// Opaque registration token (docs/24): a Project-signed gatekeeper invite
    /// or the operator's bootstrap token. Forwarded verbatim to the server,
    /// which evaluates it; the client never parses it. `None` for unauthenticated
    /// (open-registration) signups.
    pub invite_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub did: String,
    pub session_token: String,
    pub expires_at: String,
}

// ── Account info ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AccountInfoResponse {
    pub did: String,
    pub display_name: Option<String>,
    pub is_bot: bool,
}

/// One of a recipient account's active devices, with the registration id the
/// server currently holds for it. Senders compare this against the
/// registration id baked into their local session to detect a peer that
/// re-registered the device (and thus a stale session to re-establish).
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceRegistration {
    pub device_id: i32,
    pub registration_id: i32,
}

// ── Authentication ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ChallengeResponse {
    pub nonce: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub session_token: String,
    pub expires_at: String,
}

// ── Prekeys ──────────────────────────────────────────────────────────────────

/// `(id, public_key, signature)` triple for a Kyber prekey upload.
pub type KyberPreKeyTriple = (i32, Vec<u8>, Vec<u8>);

/// Upload prekeys. All fields optional — upload only what you need to refresh.
pub struct UploadPrekeysRequest {
    /// (id, public_key, signature)
    pub signed_prekey: Option<(i32, Vec<u8>, Vec<u8>)>,
    /// Vec of (id, public_key)
    pub one_time_prekeys: Option<Vec<(i32, Vec<u8>)>>,
    /// (id, public_key, signature) — last-resort Kyber prekey
    pub kyber_prekey: Option<(i32, Vec<u8>, Vec<u8>)>,
    /// Vec of (id, public_key, signature) — one-time Kyber prekeys
    pub one_time_kyber_prekeys: Option<Vec<KyberPreKeyTriple>>,
}

/// Decoded prekey bundle — bytes, not base64.
pub struct PreKeyBundleResponse {
    pub identity_key: Vec<u8>,
    pub registration_id: i32,
    pub signed_prekey_id: i32,
    pub signed_prekey_public: Vec<u8>,
    pub signed_prekey_signature: Vec<u8>,
    pub one_time_prekey: Option<(i32, Vec<u8>)>,
    pub kyber_prekey_id: i32,
    pub kyber_prekey_public: Vec<u8>,
    pub kyber_prekey_signature: Vec<u8>,
}

#[derive(Deserialize)]
pub(crate) struct RawPreKeyBundleResponse {
    identity_key: String,
    registration_id: i32,
    signed_prekey: RawSignedPreKey,
    one_time_prekey: Option<RawOneTimePreKey>,
    kyber_prekey: RawKyberPreKey,
}

#[derive(Deserialize)]
struct RawSignedPreKey {
    id: i32,
    public_key: String,
    signature: String,
}

#[derive(Deserialize)]
struct RawOneTimePreKey {
    id: i32,
    public_key: String,
}

#[derive(Deserialize)]
struct RawKyberPreKey {
    id: i32,
    public_key: String,
    signature: String,
}

impl RawPreKeyBundleResponse {
    pub(crate) fn decode(self) -> Result<PreKeyBundleResponse, NetError> {
        Ok(PreKeyBundleResponse {
            identity_key: decode_b64(&self.identity_key)?,
            registration_id: self.registration_id,
            signed_prekey_id: self.signed_prekey.id,
            signed_prekey_public: decode_b64(&self.signed_prekey.public_key)?,
            signed_prekey_signature: decode_b64(&self.signed_prekey.signature)?,
            one_time_prekey: self.one_time_prekey.map(|k| {
                Ok::<_, NetError>((k.id, decode_b64(&k.public_key)?))
            }).transpose()?,
            kyber_prekey_id: self.kyber_prekey.id,
            kyber_prekey_public: decode_b64(&self.kyber_prekey.public_key)?,
            kyber_prekey_signature: decode_b64(&self.kyber_prekey.signature)?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PrekeyStatusResponse {
    pub one_time_remaining: i64,
    pub kyber_remaining: i64,
}

// ── Messages ─────────────────────────────────────────────────────────────────

/// An outbound message to send via the server.
pub struct OutboundMessage {
    pub recipient_did: String,
    pub recipient_device_id: i32,
    /// The registration_id stored in the local session for this recipient device.
    /// The server rejects the message with 409 if this doesn't match the current
    /// registration_id, signalling that the session is stale.
    pub destination_registration_id: i32,
    pub ciphertext: Vec<u8>,
    pub message_kind: i16,
    /// Per-message expiry in seconds. `None` means use the server's global default.
    pub expiry_secs: Option<i64>,
}

/// An inbound message received from the server.
pub struct InboundMessage {
    pub id: i64,
    pub ciphertext: Vec<u8>,
    pub message_kind: i16,
    pub enqueued_at: String,
    pub sender_did: Option<String>,
    pub sender_device_id: Option<i32>,
}

#[derive(Deserialize)]
pub(crate) struct RawFetchResponse {
    messages: Vec<RawInboundMessage>,
}

#[derive(Deserialize)]
struct RawInboundMessage {
    id: i64,
    ciphertext: String,
    message_kind: i16,
    enqueued_at: String,
    sender_did: Option<String>,
    sender_device_id: Option<i32>,
}

impl RawFetchResponse {
    pub(crate) fn decode(self) -> Result<Vec<InboundMessage>, NetError> {
        self.messages.into_iter().map(|m| {
            Ok(InboundMessage {
                id: m.id,
                ciphertext: decode_b64(&m.ciphertext)?,
                message_kind: m.message_kind,
                enqueued_at: m.enqueued_at,
                sender_did: m.sender_did,
                sender_device_id: m.sender_device_id,
            })
        }).collect()
    }
}

// ── Invites ─────────────────────────────────────────────────────────────────

/// Response from validating an invite token.
#[derive(Debug, Deserialize)]
pub struct InviteValidationResponse {
    pub server_name: String,
    pub post_onboarding_redirect: Option<String>,
    /// Operator's privacy policy URL, bundled by the server so onboarding needs
    /// no separate `/v1/info` call. Absent when the operator configured none.
    #[serde(default)]
    pub privacy_policy_url: Option<String>,
}

// ── Projects ────────────────────────────────────────────────────────────────

/// A Project installed on the homeserver.
#[derive(Debug, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub url: String,
    pub description: String,
    /// OAuth login client id (docs/25), if this Project supports login.
    #[serde(default)]
    pub client_id: Option<String>,
    /// Server-vouched official flag (docs/54).
    #[serde(default)]
    pub official: bool,
}

/// Response from requesting a Project token.
#[derive(Debug, Deserialize)]
pub struct ProjectTokenResponse {
    pub token: String,
    pub expires_at: String,
}

/// Response from minting an OAuth authorization code (docs/25, same-device
/// front-end): the code the app hands back to the Project via the redirect.
#[derive(Debug, Deserialize)]
pub(crate) struct OauthAuthorizeCodeResponse {
    pub code: String,
}

/// Response from approving a device-grant login (docs/25, cross-device
/// front-end): the Project URL the user just signed in to.
#[derive(Debug, Deserialize)]
pub(crate) struct OauthDeviceApproveResponse {
    pub project_url: String,
}

// ── Recovery ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RecoveryBlobResponse {
    pub recovery_blob: String, // base64
    #[serde(default)]
    pub device_ids: Vec<i32>,
}

/// Decoded result of `get_recovery_blob` — the encrypted blob bytes plus the
/// account's currently active device_ids, used by the recovery flow to target
/// the old device for replacement.
#[derive(Debug)]
pub struct RecoveryBundle {
    pub blob: Vec<u8>,
    pub device_ids: Vec<i32>,
}

// ── Device replacement ──────────────────────────────────────────────────────

pub struct ReplaceDeviceRequest {
    pub did: String,
    pub old_device_id: i32,
    pub new_device_id: i32,
    pub new_identity_key: Vec<u8>,
    pub new_registration_id: i32,
    pub nonce: String,
    pub rotation_key_signature: Vec<u8>,
    pub rotation_key: Vec<u8>,
    pub signed_prekey_id: i32,
    pub signed_prekey_public: Vec<u8>,
    pub signed_prekey_signature: Vec<u8>,
    pub one_time_prekeys: Vec<(i32, Vec<u8>)>,
    pub kyber_prekey_id: i32,
    pub kyber_prekey_public: Vec<u8>,
    pub kyber_prekey_signature: Vec<u8>,
    pub recovery_blob: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceDeviceResponse {
    pub session_token: String,
    pub expires_at: String,
}

// ── Device linking (additive add-device, docs/04 §4) ────────────────────────

/// An additive add-device request. Mirrors [`ReplaceDeviceRequest`] minus
/// `old_device_id` (nothing is deleted) and `recovery_blob` (linking does not
/// touch it). Authorized by a rotation-key signature over
/// `"linkdevice:{did}:{new_device_id}:{nonce}"`.
pub struct LinkDeviceRequest {
    pub did: String,
    pub new_device_id: i32,
    pub new_identity_key: Vec<u8>,
    pub new_registration_id: i32,
    pub nonce: String,
    pub rotation_key_signature: Vec<u8>,
    pub rotation_key: Vec<u8>,
    pub signed_prekey_id: i32,
    pub signed_prekey_public: Vec<u8>,
    pub signed_prekey_signature: Vec<u8>,
    pub one_time_prekeys: Vec<(i32, Vec<u8>)>,
    pub kyber_prekey_id: i32,
    pub kyber_prekey_public: Vec<u8>,
    pub kyber_prekey_signature: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub struct LinkDeviceResponse {
    pub session_token: String,
    pub expires_at: String,
    pub device_id: i32,
}

// ── Provisioning mailbox (docs/04 §4) ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ProvisioningSession {
    pub session_id: String,
    pub expires_at: String,
}

fn decode_b64(s: &str) -> Result<Vec<u8>, NetError> {
    BASE64_STANDARD.decode(s).map_err(|e| NetError::Base64(e.to_string()))
}
