//! Project-signed invite/registration token format + verification (docs/24).
//!
//! The server admits a closed-registration account only if it presents a token
//! a gatekeeper Project signed. The server **never calls the Project**: it pins
//! the Project's Ed25519 public key (registered when `registration.gatekeeper`
//! is granted) and verifies the signature locally, failing closed.
//!
//! # Wire format
//!
//! A token is `base64url(JSON)` of an *envelope*:
//!
//! ```json
//! { "server_url": "...", "iss": "<project-slug>",
//!   "claims": "<base64url(claims-json)>", "sig": "<base64url(ed25519 sig)>" }
//! ```
//!
//! The top-level `server_url`/`iss` are **untrusted hints** — the substrate
//! reads `server_url` to know which server to call (docs/51), and the server
//! reads `iss` to pick which pinned key to verify against. Authority lives only
//! in the signed `claims`:
//!
//! ```json
//! { "server_url": "...", "iss": "<project-slug>", "exp": <unix-secs>,
//!   "jti": "<unique-id>", "purpose": "invite", "routing": { ... } }
//! ```
//!
//! The signature covers the exact `claims` base64url string, so there is no
//! cross-language JSON-canonicalization hazard. `purpose` lets one signing key
//! serve multiple token kinds safely (the server gates by purpose → capability,
//! not by the key) — `"invite"` for human onboarding today, room for `"bot"`
//! signup later.

use base64::prelude::*;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// `purpose` value for a human-onboarding invite (the only kind accepted on the
/// registration path today).
pub const PURPOSE_INVITE: &str = "invite";

/// A bootstrap token: the operator's setup-time shared secret, optionally
/// naming a Project to link the new account into (e.g. the superuser Project).
/// Unsigned — the secret itself is the credential. Honored only while the
/// shared secret is configured and no gatekeeper is installed (docs/24).
#[derive(Debug, Deserialize)]
pub struct BootstrapToken {
    #[allow(dead_code)]
    #[serde(rename = "s")]
    pub server_url: String,
    #[serde(rename = "k")]
    pub bootstrap_secret: String,
    /// Slug of a Project to link the new account into. `None` = register as a
    /// plain account. Naming the superuser Project is how the operator/adminbot
    /// bootstraps superuser authority.
    #[serde(rename = "p", default)]
    pub project: Option<String>,
}

/// A registration token, in one of its two shapes.
pub enum ParsedToken {
    /// A Project-signed gatekeeper invite (verified against a pinned key).
    Gatekeeper(InviteEnvelope),
    /// The operator's shared-secret bootstrap token.
    Bootstrap(BootstrapToken),
}

/// Decode and classify a registration token. Wire keys are single-char to keep
/// tokens (and their QR codes) compact: a signed gatekeeper envelope is
/// recognized by its `g` (sig) / `c` (claims) fields; a bootstrap token by `k`
/// (bootstrap_secret). Anything else is malformed.
pub fn parse(token: &str) -> Result<ParsedToken, TokenError> {
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(token.trim())
        .map_err(|_| TokenError::Malformed("invalid base64url".into()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| TokenError::Malformed("invalid JSON".into()))?;

    if value.get("g").is_some() && value.get("c").is_some() {
        let env: InviteEnvelope = serde_json::from_value(value)
            .map_err(|_| TokenError::Malformed("invalid gatekeeper envelope".into()))?;
        Ok(ParsedToken::Gatekeeper(env))
    } else if value.get("k").is_some() {
        let boot: BootstrapToken = serde_json::from_value(value)
            .map_err(|_| TokenError::Malformed("invalid bootstrap token".into()))?;
        Ok(ParsedToken::Bootstrap(boot))
    } else {
        Err(TokenError::Malformed("unrecognized token shape".into()))
    }
}

/// Constant-time string comparison, to avoid leaking the shared secret via
/// timing. Length difference short-circuits (lengths aren't secret).
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The untrusted outer envelope. Wire keys are single-char (see [`parse`]).
#[derive(Debug, Deserialize)]
pub struct InviteEnvelope {
    #[allow(dead_code)]
    #[serde(rename = "s")]
    pub server_url: String,
    #[serde(rename = "i")]
    pub iss: String,
    /// base64url(claims JSON) — the exact bytes the signature covers.
    #[serde(rename = "c")]
    pub claims: String,
    /// base64url(Ed25519 signature over the `claims` string bytes).
    #[serde(rename = "g")]
    pub sig: String,
}

/// The signed claims — the authoritative content. Wire keys are single-char.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InviteClaims {
    #[serde(rename = "s")]
    pub server_url: String,
    #[serde(rename = "i")]
    pub iss: String,
    /// Expiry, unix epoch seconds.
    #[serde(rename = "e")]
    pub exp: i64,
    /// Unique token id; single-use redemption key.
    #[serde(rename = "j")]
    pub jti: String,
    #[serde(rename = "u")]
    pub purpose: String,
    /// Opaque routing payload the gatekeeper controls; carried through to the
    /// `AccountJoinedEvent` for the post-join router. The server does not
    /// interpret it.
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<serde_json::Value>,
}

/// Why a token was rejected. The route maps these to HTTP statuses: structural
/// problems are 400 (BadRequest); admission failures (signature/issuer/expiry/
/// purpose/server) are 403 (Forbidden, fail-closed).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("malformed token: {0}")]
    Malformed(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("issuer mismatch")]
    IssuerMismatch,
    #[error("token expired")]
    Expired,
    #[error("token is for a different server")]
    WrongServer,
    #[error("unexpected token purpose")]
    WrongPurpose,
}

/// Decode the outer envelope from the base64url token string.
pub fn parse_envelope(token: &str) -> Result<InviteEnvelope, TokenError> {
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(token.trim())
        .map_err(|_| TokenError::Malformed("invalid base64url".into()))?;
    serde_json::from_slice(&bytes).map_err(|_| TokenError::Malformed("invalid envelope JSON".into()))
}

/// Verify the envelope's signature against `public_key` and decode + check the
/// claims. Returns the validated claims on success.
///
/// Checks, in order: Ed25519 signature over the `claims` bytes; `claims.iss`
/// equals the envelope `iss` (so the untrusted hint can't redirect to a
/// different signed issuer); `server_url`; `purpose`; expiry against `now`.
pub fn verify_claims(
    envelope: &InviteEnvelope,
    public_key: &[u8],
    expected_server_url: &str,
    expected_purpose: &str,
    now_unix: i64,
) -> Result<InviteClaims, TokenError> {
    // Verifying key (32 bytes).
    let key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| TokenError::Malformed("signing key not 32 bytes".into()))?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|_| TokenError::InvalidSignature)?;

    // Signature (64 bytes).
    let sig_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&envelope.sig)
        .map_err(|_| TokenError::Malformed("invalid base64url sig".into()))?;
    let signature =
        Signature::from_slice(&sig_bytes).map_err(|_| TokenError::Malformed("bad sig length".into()))?;

    // Signature covers the exact `claims` base64url string bytes.
    verifying_key
        .verify(envelope.claims.as_bytes(), &signature)
        .map_err(|_| TokenError::InvalidSignature)?;

    // Decode the now-authenticated claims.
    let claims_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&envelope.claims)
        .map_err(|_| TokenError::Malformed("invalid base64url claims".into()))?;
    let claims: InviteClaims = serde_json::from_slice(&claims_bytes)
        .map_err(|_| TokenError::Malformed("invalid claims JSON".into()))?;

    if claims.iss != envelope.iss {
        return Err(TokenError::IssuerMismatch);
    }
    if claims.server_url.trim_end_matches('/') != expected_server_url.trim_end_matches('/') {
        return Err(TokenError::WrongServer);
    }
    if claims.purpose != expected_purpose {
        return Err(TokenError::WrongPurpose);
    }
    if claims.exp <= now_unix {
        return Err(TokenError::Expired);
    }

    Ok(claims)
}

/// Current unix epoch seconds (wall clock). Isolated so the route stays terse.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Issue a token by signing `claims` with `signing_key`. This is what a
/// gatekeeper Project does; the server only verifies. Exposed for tests (and
/// any in-tree gatekeeper) so the canonical format lives in one place.
pub fn issue(signing_key: &ed25519_dalek::SigningKey, claims: &InviteClaims) -> String {
    use ed25519_dalek::Signer;
    let claims_json = serde_json::to_vec(claims).expect("claims serialize");
    let claims_b64 = BASE64_URL_SAFE_NO_PAD.encode(&claims_json);
    let sig = signing_key.sign(claims_b64.as_bytes());
    let envelope = serde_json::json!({
        "s": claims.server_url,
        "i": claims.iss,
        "c": claims_b64,
        "g": BASE64_URL_SAFE_NO_PAD.encode(sig.to_bytes()),
    });
    BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&envelope).expect("envelope serialize"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    const SERVER: &str = "http://localhost:3000";

    fn signer() -> SigningKey {
        // Deterministic key from fixed seed bytes — no rng dependency.
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn claims(exp: i64, purpose: &str) -> InviteClaims {
        InviteClaims {
            server_url: SERVER.into(),
            iss: "vetting".into(),
            exp,
            jti: "tok-1".into(),
            purpose: purpose.into(),
            routing: Some(serde_json::json!({ "audience": "northeast" })),
        }
    }

    #[test]
    fn round_trip_valid() {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        let env = parse_envelope(&token).unwrap();
        let got = verify_claims(&env, &pk, SERVER, PURPOSE_INVITE, 500).unwrap();
        assert_eq!(got.jti, "tok-1");
        assert_eq!(got.iss, "vetting");
        assert_eq!(got.routing.unwrap()["audience"], "northeast");
    }

    #[test]
    fn expired_rejected() {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        let env = parse_envelope(&token).unwrap();
        assert_eq!(
            verify_claims(&env, &pk, SERVER, PURPOSE_INVITE, 1_000),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn wrong_purpose_rejected() {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, "bot"));
        let env = parse_envelope(&token).unwrap();
        assert_eq!(
            verify_claims(&env, &pk, SERVER, PURPOSE_INVITE, 500),
            Err(TokenError::WrongPurpose)
        );
    }

    #[test]
    fn wrong_key_rejected() {
        let sk = signer();
        let other_pk = SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        let env = parse_envelope(&token).unwrap();
        assert_eq!(
            verify_claims(&env, &other_pk, SERVER, PURPOSE_INVITE, 500),
            Err(TokenError::InvalidSignature)
        );
    }

    #[test]
    fn tampered_claims_rejected() {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        let mut env = parse_envelope(&token).unwrap();
        // Swap in a different claims blob; signature no longer matches.
        let forged = BASE64_URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&claims(9_999, PURPOSE_INVITE)).unwrap(),
        );
        env.claims = forged;
        assert_eq!(
            verify_claims(&env, &pk, SERVER, PURPOSE_INVITE, 500),
            Err(TokenError::InvalidSignature)
        );
    }

    #[test]
    fn issuer_mismatch_rejected() {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes();
        let token = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        let mut env = parse_envelope(&token).unwrap();
        env.iss = "someone-else".into(); // hint disagrees with signed claims.iss
        assert_eq!(
            verify_claims(&env, &pk, SERVER, PURPOSE_INVITE, 500),
            Err(TokenError::IssuerMismatch)
        );
    }

    #[test]
    fn malformed_token_rejected() {
        assert!(matches!(
            parse_envelope("!!!not base64!!!"),
            Err(TokenError::Malformed(_))
        ));
    }

    #[test]
    fn parse_classifies_gatekeeper_vs_bootstrap() {
        // Gatekeeper: has sig + claims.
        let sk = signer();
        let gk = issue(&sk, &claims(1_000, PURPOSE_INVITE));
        assert!(matches!(parse(&gk).unwrap(), ParsedToken::Gatekeeper(_)));

        // Bootstrap: has `k` (bootstrap_secret).
        let boot = BASE64_URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "s": SERVER, "k": "s3cret", "p": "adminbot"
            }))
            .unwrap(),
        );
        match parse(&boot).unwrap() {
            ParsedToken::Bootstrap(b) => {
                assert_eq!(b.bootstrap_secret, "s3cret");
                assert_eq!(b.project.as_deref(), Some("adminbot"));
            }
            _ => panic!("expected bootstrap token"),
        }

        // Neither shape → malformed.
        let other = BASE64_URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&serde_json::json!({ "s": SERVER })).unwrap());
        assert!(matches!(parse(&other), Err(TokenError::Malformed(_))));
    }

    #[test]
    fn secret_eq_compares_values() {
        assert!(secret_eq("abc", "abc"));
        assert!(!secret_eq("abc", "abd"));
        assert!(!secret_eq("abc", "abcd")); // length differs
        assert!(!secret_eq("", "x"));
        assert!(secret_eq("", ""));
    }
}
