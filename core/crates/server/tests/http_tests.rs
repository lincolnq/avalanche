//! HTTP-level integration tests for the DID resolution endpoint.
//!
//! Uses tower's `oneshot` to drive the full Axum handler stack in-process.
//! Requires `TEST_DATABASE_URL` to be set and the schema to be applied.
//!
//! Unlike `db_tests.rs`, these tests cannot use the transaction-rollback
//! pattern because handlers manage their own connections. Each registration
//! call generates a unique DID (nanosecond timestamp entropy), so leftover
//! rows are benign.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::prelude::*;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use server::{config::Config, routes, state::AppState};

async fn test_state() -> AppState {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run server tests");
    let pool = PgPool::connect(&url).await.expect("failed to connect to test database");
    let config = Config {
        database_url: url,
        bind_addr: "0.0.0.0:0".into(),
        server_url: "http://localhost:3000".into(),
        token_lifetime_secs: 86400,
        message_expiry_secs: 30 * 86400,
        message_expiry_min_secs: 300,
        message_expiry_max_secs: 30 * 86400,
        prekey_low_threshold: 10,
        project_token_lifetime_secs: 3600,
        projects_json: "[]".into(),
        relay_url: None,
        server_name: "Test".into(),
        invite_domain: "go.example.test".into(),
    };
    AppState::new(pool, config)
}

/// Register a dummy account and return the parsed response body.
///
/// Registers as a bot so the server generates a local DID and skips the
/// PLC-directory verification + identity-key-signature checks. The auth tests
/// that consume this helper exercise endpoints that are bot/human-agnostic.
async fn register_dummy(app: &axum::Router) -> Value {
    let body = serde_json::json!({
        "identity_key":     BASE64_STANDARD.encode([1u8; 32]),
        "registration_id":  1,
        "device_id":        1,
        "signed_prekey": {
            "id":         1,
            "public_key": BASE64_STANDARD.encode([2u8; 32]),
            "signature":  BASE64_STANDARD.encode([3u8; 64]),
        },
        "one_time_prekeys": [{ "id": 1, "public_key": BASE64_STANDARD.encode([4u8; 32]) }],
        "kyber_prekey": {
            "id":         1,
            "public_key": BASE64_STANDARD.encode([5u8; 32]),
            "signature":  BASE64_STANDARD.encode([6u8; 64]),
        },
        "is_bot": true,
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED, "registration must succeed");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── DID resolution endpoint tests ────────────────────────────────────────────

#[tokio::test]
async fn resolve_did_returns_document() {
    let app = routes::router().with_state(test_state().await);

    let reg = register_dummy(&app).await;
    let did = reg["did"].as_str().unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/.well-known/did/{did}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(doc["id"], did);
    assert_eq!(doc["verificationMethod"][0]["controller"], did);
    assert_eq!(doc["service"][0]["type"], "AvalancheHomeserver");
    assert_eq!(doc["service"][0]["serviceEndpoint"], "http://localhost:3000");
}

#[tokio::test]
async fn resolve_unknown_did_returns_404() {
    let app = routes::router().with_state(test_state().await);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/.well-known/did/did:plc:doesnotexist0000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Auth challenge / token tests ──────────────────────────────────────────────

/// Register a device with a real Ed25519 keypair, return (did, keypair).
async fn register_with_keypair(
    app: &axum::Router,
) -> (String, libsignal_protocol::IdentityKeyPair) {
    use libsignal_protocol as signal;
    use rand::TryRngCore as _;

    let keypair = signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err());
    let identity_key_b64 = BASE64_STANDARD.encode(keypair.identity_key().serialize());

    // Bot path: server generates a local DID and skips PLC verification, so the
    // test can focus on the challenge-response flow.
    let body = serde_json::json!({
        "identity_key":    identity_key_b64,
        "registration_id": 1,
        "device_id":       1,
        "signed_prekey":   { "id": 1, "public_key": BASE64_STANDARD.encode([2u8; 32]), "signature": BASE64_STANDARD.encode([3u8; 64]) },
        "one_time_prekeys":[{ "id": 1, "public_key": BASE64_STANDARD.encode([4u8; 32]) }],
        "kyber_prekey":    { "id": 1, "public_key": BASE64_STANDARD.encode([5u8; 32]), "signature": BASE64_STANDARD.encode([6u8; 64]) },
        "is_bot": true,
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let reg: Value = serde_json::from_slice(&bytes).unwrap();
    (reg["did"].as_str().unwrap().to_string(), keypair)
}

/// Request a challenge for (did, device_id=1), sign it, return the token body.
async fn get_challenge_and_sign(
    app: &axum::Router,
    did: &str,
    keypair: &libsignal_protocol::IdentityKeyPair,
) -> Value {
    use rand::TryRngCore as _;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/challenge")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "did": did, "device_id": 1 }))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let nonce = serde_json::from_slice::<Value>(&bytes).unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();

    let nonce_bytes = BASE64_URL_SAFE_NO_PAD.decode(&nonce).unwrap();
    let sig = keypair
        .private_key()
        .calculate_signature(&nonce_bytes, &mut rand::rngs::OsRng.unwrap_err())
        .expect("sign");

    serde_json::json!({
        "did":       did,
        "device_id": 1,
        "nonce":     nonce,
        "signature": BASE64_URL_SAFE_NO_PAD.encode(sig),
    })
}

#[tokio::test]
async fn auth_challenge_response_issues_token() {
    let app = routes::router().with_state(test_state().await);
    let (did, keypair) = register_with_keypair(&app).await;
    let token_body = get_challenge_and_sign(&app, &did, &keypair).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&token_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!body["session_token"].as_str().unwrap_or("").is_empty());
    assert!(!body["expires_at"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn auth_token_replay_returns_401() {
    let app = routes::router().with_state(test_state().await);
    let (did, keypair) = register_with_keypair(&app).await;
    let token_body = get_challenge_and_sign(&app, &did, &keypair).await;

    let post = |body: &Value| {
        Request::builder()
            .method("POST")
            .uri("/v1/auth/token")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    };

    let resp1 = app.clone().oneshot(post(&token_body)).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK, "first use must succeed");

    let resp2 = app.clone().oneshot(post(&token_body)).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNAUTHORIZED, "replay must be rejected");
}

#[tokio::test]
async fn auth_token_wrong_signature_returns_401() {
    let app = routes::router().with_state(test_state().await);
    let (did, keypair) = register_with_keypair(&app).await;
    let mut token_body = get_challenge_and_sign(&app, &did, &keypair).await;

    // Corrupt the signature.
    token_body["signature"] = Value::String(BASE64_URL_SAFE_NO_PAD.encode([0u8; 64]));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&token_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_token_without_nonce_returns_422() {
    let app = routes::router().with_state(test_state().await);

    // Old format: did + device_id only — missing required nonce and signature fields.
    let body = serde_json::json!({ "did": "did:plc:doesnotmatter00000000", "device_id": 1 });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
