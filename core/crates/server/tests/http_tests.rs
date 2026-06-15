//! HTTP-level integration tests for the DID resolution endpoint.
//!
//! Uses tower's `oneshot` to drive the full Axum handler stack in-process.
//! Requires `TEST_DATABASE_URL` to be set. Schema migrations are applied
//! automatically on first connect via the same embedded migrator the server
//! binary uses.
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
use tokio::sync::OnceCell;
use tower::ServiceExt;

use crypto::groups::ServerSecretParams;
use crypto::sender_cert::SenderCertChain;
use server::{config::Config, db, routes, state::AppState};

static SETUP: OnceCell<()> = OnceCell::const_new();

/// Apply migrations once across the whole test binary and disable the per-IP
/// rate limit (tests share a long-lived dev DB; accumulated counters across
/// runs would otherwise trip the registration limit). Each test still gets
/// its own `PgPool` so connection budgets don't contend across parallel tests.
async fn ensure_setup(url: &str) {
    SETUP
        .get_or_init(|| async {
            unsafe { std::env::set_var("ACTNET_DISABLE_IP_RATE_LIMITS", "1") };
            let pool = PgPool::connect(url).await.expect("failed to connect to test database");
            server::migrate::run(&pool).await.expect("failed to apply test migrations");
        })
        .await;
}

async fn test_state() -> AppState {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run server tests");
    ensure_setup(&url).await;
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
        adminbot_did: String::new(),
    };
    // Load (or seed) the group crypto bundle exactly as `main.rs` does — a
    // bincoded `GroupCryptoBundle` under the current version. Seeding the raw
    // `ServerSecretParams` here instead would collide with a real server's row
    // on a shared DB (same `version` key, incompatible bytes), so this must
    // mirror production to work against any database, not just a pristine one.
    let mut conn = pool.acquire().await.expect("acquire");
    let bytes = db::zkgroup_params::load_or_init(
        &mut conn,
        db::zkgroup_params::CURRENT_VERSION,
        || {
            db::zkgroup_params::GroupCryptoBundle {
                zkgroup_secret: ServerSecretParams::generate().to_bytes(),
                sender_cert_chain: SenderCertChain::generate()
                    .expect("generate sender cert chain")
                    .to_bytes(),
            }
            .to_bytes()
        },
    )
    .await
    .expect("load zkgroup params");
    drop(conn);
    let bundle = db::zkgroup_params::GroupCryptoBundle::from_bytes(&bytes)
        .expect("stored group crypto bundle is corrupt");
    let zkgroup_secret =
        ServerSecretParams::from_bytes(&bundle.zkgroup_secret).expect("decode params");
    let sender_cert_chain =
        SenderCertChain::from_bytes(&bundle.sender_cert_chain).expect("decode sender cert chain");
    AppState::new(pool, config, zkgroup_secret, sender_cert_chain)
}

/// Register a dummy account and return the parsed response body.
///
/// Registers as a bot so the server generates a local DID and skips the
/// PLC-directory verification + identity-key-signature checks. The auth tests
/// that consume this helper exercise endpoints that are bot/human-agnostic.
async fn register_dummy(app: &axum::Router) -> Value {
    // Random identity_key so parallel tests don't collide on the
    // (identity_key, nanos)-derived DID generated for bot accounts.
    use rand::TryRngCore as _;
    let mut ik = [0u8; 32];
    rand::rngs::OsRng.unwrap_err().try_fill_bytes(&mut ik).unwrap();
    let body = serde_json::json!({
        "identity_key":     BASE64_STANDARD.encode(ik),
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

// ── Stale device detection tests ─────────────────────────────────────────────

/// Happy path: sender supplies the correct `destination_registration_id` → 200 OK.
#[tokio::test]
async fn send_correct_registration_id_returns_200() {
    let app = routes::router().with_state(test_state().await);

    // Alice (sender) — bot registration returns session_token directly.
    let alice = register_dummy(&app).await;
    let alice_token = alice["session_token"].as_str().unwrap().to_string();

    // Bob (recipient) — registered with registration_id=1 and device_id=1.
    let bob = register_dummy(&app).await;
    let bob_did = bob["did"].as_str().unwrap().to_string();

    let body = serde_json::json!({
        "messages": [{
            "recipient_did": bob_did,
            "recipient_device_id": 1,
            "destination_registration_id": 1,   // matches Bob's stored registration_id
            "ciphertext": BASE64_STANDARD.encode([0u8; 32]),
            "message_kind": 1,
        }]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {alice_token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Error path: sender supplies a wrong `destination_registration_id` → 409 Conflict.
#[tokio::test]
async fn send_stale_registration_id_returns_409() {
    let app = routes::router().with_state(test_state().await);

    let alice = register_dummy(&app).await;
    let alice_token = alice["session_token"].as_str().unwrap().to_string();

    let bob = register_dummy(&app).await;
    let bob_did = bob["did"].as_str().unwrap().to_string();

    let body = serde_json::json!({
        "messages": [{
            "recipient_did": bob_did,
            "recipient_device_id": 1,
            "destination_registration_id": 9999,    // wrong — Bob's actual registration_id is 1
            "ciphertext": BASE64_STANDARD.encode([0u8; 32]),
            "message_kind": 1,
        }]
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {alice_token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let resp_body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp_body["error"], "stale_device");
    assert_eq!(resp_body["stale_devices"][0]["device_id"], 1);
    assert_eq!(resp_body["stale_devices"][0]["did"], bob_did);
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

// ── Account deletion tests ────────────────────────────────────────────────────

/// Helper: register a keypair account and get a session token. Returns (did, token).
async fn register_and_get_token(app: &axum::Router) -> (String, String) {
    let (did, keypair) = register_with_keypair(app).await;
    let token_body = get_challenge_and_sign(app, &did, &keypair).await;

    let resp = app
        .clone()
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
    assert_eq!(resp.status(), StatusCode::OK, "token issuance must succeed");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let token = body["session_token"].as_str().unwrap().to_string();
    (did, token)
}

#[tokio::test]
async fn delete_account_removes_all_data() {
    let app = routes::router().with_state(test_state().await);
    let (did, token) = register_and_get_token(&app).await;

    // 1. DELETE /v1/accounts → 204 No Content.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/accounts")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "deletion must return 204");

    // 2. DID document should now return 404.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/.well-known/did/{did}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "DID must be gone after deletion");

    // 3. A subsequent authenticated request with the same token must return 401
    //    because the session token was deleted along with the account.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/accounts/{did}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "deleted session token must return 401");
}

#[tokio::test]
async fn get_groups_server_params_returns_decodable_public_params() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/groups/server-params")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(body["version"], db::zkgroup_params::CURRENT_VERSION);
    let encoded = body["params"].as_str().expect("params field");
    let decoded = BASE64_URL_SAFE_NO_PAD.decode(encoded).expect("base64 decode");

    // Bytes must decode as ServerPublicParams and match what the loaded
    // ServerSecretParams derives — i.e. the endpoint isn't returning stale
    // or otherwise mismatched material.
    let public = crypto::groups::ServerPublicParams::from_bytes(&decoded)
        .expect("decode ServerPublicParams");
    assert_eq!(public.to_bytes(), state.zkgroup_secret.public_params().to_bytes());
}

// ── Storage service HTTP tests (docs/05) ─────────────────────────────────────

async fn storage_put(app: &axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/storage/items")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    // Error responses are plain text, not JSON; fall back to Null so callers
    // that only check status don't panic.
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn storage_get(app: &axum::Router, token: Option<&str>, since: i64) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/storage/items?since={since}"));
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let resp = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn storage_put_then_pull_roundtrip() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let rid = BASE64_STANDARD.encode([7u8; 16]);
    let ct = BASE64_STANDARD.encode(b"hello-storage");

    let (status, body) = storage_put(
        &app,
        &token,
        serde_json::json!({
            "writes": [
                { "record_id": rid, "expected_version": 0, "deleted": false, "ciphertext": ct }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["applied"].as_array().unwrap().len(), 1);
    assert_eq!(body["conflicts"].as_array().unwrap().len(), 0);
    assert_eq!(body["applied"][0]["record_id"], rid);
    assert_eq!(body["applied"][0]["version"], 1);

    // The freshly registered account has exactly this one record.
    let (status, body) = storage_get(&app, Some(&token), 0).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["record_id"], rid);
    assert_eq!(items[0]["ciphertext"], ct);
    assert_eq!(items[0]["deleted"], false);
    assert_eq!(body["has_more"], false);
    assert_eq!(body["next_cursor"], 1);
}

#[tokio::test]
async fn storage_pull_requires_auth() {
    let app = routes::router().with_state(test_state().await);
    let (status, _) = storage_get(&app, None, 0).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn storage_cas_conflict_surfaced_in_response() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let rid = BASE64_STANDARD.encode([8u8; 16]);
    let ct = BASE64_STANDARD.encode(b"v1");
    let write = serde_json::json!({
        "writes": [
            { "record_id": rid, "expected_version": 0, "deleted": false, "ciphertext": ct }
        ]
    });

    // First create succeeds.
    let (status, body) = storage_put(&app, &token, write.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["applied"].as_array().unwrap().len(), 1);

    // Replaying expected_version=0 now conflicts (the row is at version 1).
    let (status, body) = storage_put(&app, &token, write).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["applied"].as_array().unwrap().len(), 0);
    let conflicts = body["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0]["record_id"], rid);
    assert_eq!(conflicts[0]["current_version"], 1);
}

#[tokio::test]
async fn storage_record_over_size_limit_rejected() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    // 9 KB ciphertext exceeds the 8 KB per-record cap.
    let rid = BASE64_STANDARD.encode([9u8; 16]);
    let big = BASE64_STANDARD.encode(vec![0u8; 9 * 1024]);
    let (status, _) = storage_put(
        &app,
        &token,
        serde_json::json!({
            "writes": [
                { "record_id": rid, "expected_version": 0, "deleted": false, "ciphertext": big }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── Storage snapshot HTTP tests (docs/05 §7) ─────────────────────────────────

async fn snapshot_put(app: &axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/storage/snapshot")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn snapshot_get(app: &axum::Router, token: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri("/v1/storage/snapshot");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let resp = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn storage_snapshot_put_then_get_roundtrip() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    // No snapshot yet → 404.
    let (status, _) = snapshot_get(&app, Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let blob = BASE64_STANDARD.encode(b"whole-store-snapshot");
    let (status, body) = snapshot_put(
        &app,
        &token,
        serde_json::json!({ "snapshot_version": 3, "blob": blob }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stored"], true);
    assert_eq!(body["snapshot_version"], 3);

    let (status, body) = snapshot_get(&app, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["snapshot_version"], 3);
    assert_eq!(body["blob"], blob);
}

#[tokio::test]
async fn storage_snapshot_requires_auth() {
    let app = routes::router().with_state(test_state().await);
    let (status, _) = snapshot_get(&app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn storage_snapshot_stale_push_rejected() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let v2 = BASE64_STANDARD.encode(b"v2");
    let (status, body) = snapshot_put(
        &app,
        &token,
        serde_json::json!({ "snapshot_version": 2, "blob": v2.clone() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stored"], true);

    // An older version is not stored and reports the version still held.
    let v1 = BASE64_STANDARD.encode(b"v1");
    let (status, body) = snapshot_put(
        &app,
        &token,
        serde_json::json!({ "snapshot_version": 1, "blob": v1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stored"], false);
    assert_eq!(body["snapshot_version"], 2);

    // The stored blob is still v2's.
    let (_, body) = snapshot_get(&app, Some(&token)).await;
    assert_eq!(body["blob"], v2);
}

#[tokio::test]
async fn storage_snapshot_empty_blob_rejected() {
    let app = routes::router().with_state(test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let (status, _) = snapshot_put(
        &app,
        &token,
        serde_json::json!({ "snapshot_version": 1, "blob": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
