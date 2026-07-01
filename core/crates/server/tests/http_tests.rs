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
    test_state_with(server::config::RegistrationMode::Open, None).await
}

/// Like [`test_state`] but with an explicit registration mode and shared
/// secret, for the capability / closed-registration tests.
async fn test_state_with(
    registration_mode: server::config::RegistrationMode,
    registration_shared_secret: Option<String>,
) -> AppState {
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
        oauth_auth_code_lifetime_secs: 120,
        oauth_device_code_lifetime_secs: 600,
        oauth_device_poll_interval_secs: 5,
        projects_json: "[]".into(),
        relay_url: None,
        server_name: "Test".into(),
        invite_domain: "go.example.test".into(),
        registration_mode,
        registration_shared_secret,
        privacy_policy_url: None,
        attachment_blob_dir: std::env::temp_dir()
            .join("av-test-attachment-blobs")
            .to_string_lossy()
            .into_owned(),
        attachment_blob_ttl_secs: 45 * 86400,
        attachment_max_size_bytes: 100 * 1024 * 1024,
        attachment_bytes_per_hour: 500 * 1024 * 1024,
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
async fn attachment_allocate_upload_download_delete_round_trip() {
    let app = routes::router().with_state(test_state().await);
    let (_did, token) = register_and_get_token(&app).await;

    let ciphertext = vec![0xABu8; 4096];

    // 1. Allocate a slot.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/attachments")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "size_bytes": ciphertext.len() }))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let attachment_id = body["attachment_id"].as_str().unwrap().to_string();
    assert!(body["expires_at_ms"].as_i64().unwrap() > 0);

    // The upload descriptor is backend-agnostic: a PUT to an absolute url with
    // replayed headers (incl. the echoed bearer). The client replays it blindly.
    assert_eq!(body["upload"]["method"], "PUT");
    assert!(body["upload"]["url"].as_str().unwrap().ends_with(&format!("/v1/attachments/{attachment_id}")));
    let hdrs = body["upload"]["headers"].as_array().unwrap();
    let has_authz = hdrs.iter().any(|h| h[0] == "authorization" && h[1] == format!("Bearer {token}"));
    assert!(has_authz, "upload descriptor must echo the bearer for the LocalFs backend");
    assert!(body["download_url"].as_str().unwrap().ends_with(&format!("/v1/attachments/{attachment_id}")));

    // 2. Upload the ciphertext.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/attachments/{attachment_id}"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/octet-stream")
                .body(Body::from(ciphertext.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 3. Download the ciphertext — exact bytes back. No Authorization header:
    //    download is unauthenticated, the unguessable id is the capability.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/attachments/{attachment_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let got = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(got.as_ref(), ciphertext.as_slice());

    // 4. Range request returns 206 with the requested slice (also unauth'd).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/attachments/{attachment_id}"))
                .header("range", "bytes=0-9")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let part = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(part.len(), 10);

    // 5. Delete, then download → 404.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/attachments/{attachment_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/attachments/{attachment_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn attachment_allocate_rejects_oversize() {
    let app = routes::router().with_state(test_state().await);
    let (_did, token) = register_and_get_token(&app).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/attachments")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "size_bytes": 200i64 * 1024 * 1024 }))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
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

/// A storage write nudges the account's *other* connected devices (docs/05 §8)
/// but never the writer. We register two devices for one account, inject a
/// `WsPush` channel for each into `ws_connections`, then PUT as device A and
/// assert B got a `StorageChanged` and A did not.
#[tokio::test]
async fn storage_put_nudges_other_devices_not_writer() {
    use server::state::WsPush;
    use tokio::sync::mpsc;

    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let reg = register_dummy(&app).await;
    let did = reg["did"].as_str().unwrap().to_string();
    let token = reg["session_token"].as_str().unwrap().to_string();

    // Device A is the one registration created (device_id 1). Add device B.
    let mut conn = state.db.acquire().await.unwrap();
    let dev_a = db::devices::find_by_did(&mut conn, &did, 1)
        .await
        .unwrap()
        .expect("device A exists");
    let dev_b_pk = db::devices::create(&mut conn, dev_a.account_id, 2, &[5u8; 33], 2)
        .await
        .unwrap();
    drop(conn);

    // Stand in for two live WebSockets.
    let (tx_a, mut rx_a) = mpsc::unbounded_channel::<WsPush>();
    let (tx_b, mut rx_b) = mpsc::unbounded_channel::<WsPush>();
    {
        let mut conns = state.ws_connections.write().await;
        conns.insert(dev_a.id, tx_a);
        conns.insert(dev_b_pk, tx_b);
    }

    // Write as device A's session.
    let rid = BASE64_STANDARD.encode([8u8; 16]);
    let ct = BASE64_STANDARD.encode(b"nudge-me");
    let (status, _) = storage_put(
        &app,
        &token,
        serde_json::json!({
            "writes": [{ "record_id": rid, "expected_version": 0, "ciphertext": ct }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The other device was nudged...
    match rx_b.try_recv() {
        Ok(WsPush::StorageChanged { high_seq }) => assert!(high_seq >= 1),
        other => panic!("device B expected StorageChanged, got {other:?}"),
    }
    // ...and the writer was not.
    assert!(rx_a.try_recv().is_err(), "writer must not nudge itself");
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

// ── Capability framework + closed registration (docs/22, 24) ────────────────

/// The dev/test bootstrap shared secret.
const SECRET: &str = "test-bootstrap-secret";

/// Serializes the tests that depend on the *global* "is any gatekeeper
/// installed?" state. The shared-secret bootstrap path auto-disables once any
/// Project holds `registration.gatekeeper` — a global condition — so on the
/// shared dev DB one test installing a gatekeeper would otherwise break another
/// test's superuser bootstrap. Holding this lock for the whole test keeps each
/// one's gatekeeper window exclusive.
static GATEKEEPER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// A process-unique, cross-run-unique id. Combines wall-clock nanos (distinct
/// across test runs on the shared DB) with a monotonic counter (distinct across
/// concurrent tasks within a run — `nanos()` alone collides under parallelism,
/// which was the source of an earlier flaky DID-collision failure).
fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", nanos(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Build a bootstrap registration token: base64url({s, k, p?}) with single-char
/// wire keys (s=server_url, k=bootstrap_secret, p=project). Naming a project
/// links the new account into it.
fn bootstrap_token(secret: &str, project: Option<&str>) -> String {
    let mut payload = serde_json::json!({
        "s": "http://localhost:3000",
        "k": secret,
    });
    if let Some(p) = project {
        payload["p"] = serde_json::json!(p);
    }
    BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
}

/// Register a bot account, optionally presenting an invite/bootstrap token.
/// Returns the response status and parsed body (Null for non-JSON error
/// bodies). The random identity key yields a unique server-generated DID, so
/// concurrent registrations never collide.
async fn register_bot(app: &axum::Router, invite_token: Option<&str>) -> (StatusCode, Value) {
    use rand::TryRngCore as _;
    let mut ik = [0u8; 32];
    rand::rngs::OsRng.unwrap_err().try_fill_bytes(&mut ik).unwrap();
    let mut body = serde_json::json!({
        "identity_key":     BASE64_STANDARD.encode(ik),
        "registration_id":  1,
        "device_id":        1,
        "signed_prekey":    { "id": 1, "public_key": BASE64_STANDARD.encode([2u8; 32]), "signature": BASE64_STANDARD.encode([3u8; 64]) },
        "one_time_prekeys": [{ "id": 1, "public_key": BASE64_STANDARD.encode([4u8; 32]) }],
        "kyber_prekey":     { "id": 1, "public_key": BASE64_STANDARD.encode([5u8; 32]), "signature": BASE64_STANDARD.encode([6u8; 64]) },
        "is_bot": true,
    });
    if let Some(t) = invite_token {
        body["invite_token"] = serde_json::json!(t);
    }
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
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

async fn admin_req(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let req = match body {
        Some(b) => {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::from(serde_json::to_vec(&b).unwrap())).unwrap()
        }
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

/// Stand up a server in the given mode with [`SECRET`] configured, seed the
/// (empty) superuser Project the way `main.rs` does, then bootstrap a superuser
/// by registering a bot with a bootstrap token naming the superuser Project.
/// Returns (app, superuser_did, superuser_session_token). The superuser's DID
/// is server-generated from a random key, so concurrent tests never collide.
async fn setup_adminbot(
    registration_mode: server::config::RegistrationMode,
) -> (axum::Router, String, String) {
    let state = test_state_with(registration_mode, Some(SECRET.to_string())).await;
    {
        let mut conn = state.db.acquire().await.unwrap();
        // Clear any committed gatekeeper grants so the shared-secret bootstrap
        // path is active for this (lock-serialized) test — the global
        // auto-disable check would otherwise observe rows committed by the
        // matrix test or a crashed prior run.
        sqlx::query("DELETE FROM project_capabilities WHERE capability = 'registration.gatekeeper'")
            .execute(&mut *conn)
            .await
            .unwrap();
        // main.rs seeds the superuser Project at startup; tests build AppState
        // directly, so seed it here.
        server::db::projects::ensure_adminbot_project(&mut conn, "adminbot")
            .await
            .unwrap();
    }
    let app = routes::router().with_state(state);
    let token = bootstrap_token(SECRET, Some("adminbot"));
    let (status, body) = register_bot(&app, Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "superuser bootstrap: {body:?}");
    let did = body["did"].as_str().unwrap().to_string();
    let session = body["session_token"].as_str().unwrap().to_string();
    (app, did, session)
}

#[tokio::test]
async fn admin_endpoints_require_superuser_membership() {
    let _guard = GATEKEEPER_LOCK.lock().await;
    let (app, _admin_did, admin_token) =
        setup_adminbot(server::config::RegistrationMode::Open).await;

    // The superuser (bootstrapped into the adminbot Project) can list projects.
    let (status, body) = admin_req(&app, "GET", "/v1/admin/projects", &admin_token, None).await;
    assert_eq!(status, StatusCode::OK);
    let projects = body["projects"].as_array().unwrap();
    let adminbot_proj = projects
        .iter()
        .find(|p| p["slug"] == "adminbot")
        .expect("adminbot project listed");
    assert_eq!(adminbot_proj["superuser"], true);
    assert_eq!(
        adminbot_proj["capabilities"].as_array().unwrap().len(),
        0,
        "superuser holds authority via membership, not capability rows"
    );

    // A plain bot (no project link) is rejected from admin endpoints (401).
    let (status, body) = register_bot(&app, None).await;
    assert_eq!(status, StatusCode::CREATED);
    let other_token = body["session_token"].as_str().unwrap().to_string();
    let (status, _) = admin_req(&app, "GET", "/v1/admin/projects", &other_token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn superuser_project_not_api_mutable() {
    let _guard = GATEKEEPER_LOCK.lock().await;
    let (app, _admin_did, admin_token) =
        setup_adminbot(server::config::RegistrationMode::Open).await;

    // The superuser Project's membership can't be changed via the admin API —
    // only the secret-gated bootstrap path grants superuser.
    let (status, _) = admin_req(
        &app,
        "POST",
        "/v1/admin/projects/adminbot/bots",
        &admin_token,
        Some(serde_json::json!({ "bot_did": "did:local:whatever" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nor can it be uninstalled.
    let (status, _) =
        admin_req(&app, "DELETE", "/v1/admin/projects/adminbot", &admin_token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn closed_registration_admission_matrix() {
    use ed25519_dalek::SigningKey;
    use server::invite_token::{issue, now_unix, InviteClaims};

    let _guard = GATEKEEPER_LOCK.lock().await;
    let (app, _admin_did, admin_token) =
        setup_adminbot(server::config::RegistrationMode::Closed).await;

    // No credential → rejected (fail-closed).
    let (status, _) = register_bot(&app, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Wrong shared secret → rejected.
    let (status, _) = register_bot(&app, Some(&bootstrap_token("wrong-secret", None))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Correct shared secret (no project) → admitted as a plain member, while no
    // gatekeeper is installed yet.
    let (status, _) = register_bot(&app, Some(&bootstrap_token(SECRET, None))).await;
    assert_eq!(status, StatusCode::CREATED, "shared secret must admit");

    // Install a gatekeeper Project + signing key.
    let slug = format!("gk{}", unique_id());
    let signing = SigningKey::from_bytes(&[42u8; 32]);
    let pubkey_b64 = BASE64_STANDARD.encode(signing.verifying_key().to_bytes());

    let (status, _) = admin_req(
        &app,
        "POST",
        "/v1/admin/projects",
        &admin_token,
        Some(serde_json::json!({ "slug": slug, "name": "Gatekeeper" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = admin_req(
        &app,
        "POST",
        "/v1/admin/capabilities",
        &admin_token,
        Some(serde_json::json!({
            "project_slug": slug,
            "capability": "registration.gatekeeper",
            "gatekeeper_public_key": pubkey_b64,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Auto-disable: installing a gatekeeper retires the shared secret.
    let (status, _) = register_bot(&app, Some(&bootstrap_token(SECRET, None))).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "shared secret must auto-disable once a gatekeeper exists"
    );

    let issue_token = |jti: &str, exp: i64, purpose: &str, iss: &str| {
        let claims = InviteClaims {
            server_url: "http://localhost:3000".into(),
            iss: iss.into(),
            exp,
            jti: jti.into(),
            purpose: purpose.into(),
            routing: None,
        };
        issue(&signing, &claims)
    };

    // Valid gatekeeper token → admitted.
    let jti = unique_id();
    let token = issue_token(&jti, now_unix() + 3600, "invite", &slug);
    let (status, _) = register_bot(&app, Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED, "valid token must admit");

    // Replay → single-use rejection.
    let (status, _) = register_bot(&app, Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "replayed token must be rejected");

    // Expired → rejected.
    let token_exp = issue_token(&unique_id(), now_unix() - 10, "invite", &slug);
    let (status, _) = register_bot(&app, Some(&token_exp)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Wrong purpose → rejected.
    let token_bot = issue_token(&unique_id(), now_unix() + 3600, "bot", &slug);
    let (status, _) = register_bot(&app, Some(&token_bot)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Unknown issuer → rejected.
    let token_unknown = issue_token(&unique_id(), now_unix() + 3600, "invite", "nope");
    let (status, _) = register_bot(&app, Some(&token_unknown)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Revoke the gatekeeper capability → its tokens stop working (fail-closed),
    // and the shared secret comes back (no gatekeeper installed any more).
    let (status, _) = admin_req(
        &app,
        "DELETE",
        &format!("/v1/admin/capabilities/{slug}/registration.gatekeeper"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let token_after = issue_token(&unique_id(), now_unix() + 3600, "invite", &slug);
    let (status, _) = register_bot(&app, Some(&token_after)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "revoked gatekeeper must fail closed");

    let (status, _) = register_bot(&app, Some(&bootstrap_token(SECRET, None))).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "shared secret re-enables when no gatekeeper remains"
    );
}

#[tokio::test]
async fn account_joined_catch_up() {
    let _guard = GATEKEEPER_LOCK.lock().await;
    let (app, _admin_did, admin_token) =
        setup_adminbot(server::config::RegistrationMode::Open).await;

    // Snapshot the current max event id directly so a shared DB with many prior
    // events doesn't push our new one past the fetch cap.
    let state = test_state_with(server::config::RegistrationMode::Open, None).await;
    let mut conn = state.db.acquire().await.unwrap();
    let before_max: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM server_events")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    drop(conn);

    // A new registration appends an account_joined event.
    let (status, body) = register_bot(&app, None).await;
    assert_eq!(status, StatusCode::CREATED);
    let new_did = body["did"].as_str().unwrap().to_string();

    // The superuser (subscribe.account_joined via the pin) can read it.
    let (status, body) = admin_req(
        &app,
        "GET",
        &format!("/v1/admin/events?since={before_max}&kind=account_joined"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = body["events"].as_array().unwrap();
    assert!(
        events.iter().any(|e| e["did"] == new_did),
        "catch-up must include the new account_joined event"
    );

    // A bot without the capability is forbidden from the catch-up endpoint.
    let (status, body) = register_bot(&app, None).await;
    assert_eq!(status, StatusCode::CREATED);
    let plain_token = body["session_token"].as_str().unwrap().to_string();
    let (status, _) =
        admin_req(&app, "GET", "/v1/admin/events", &plain_token, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── Abuse report (docs/12 §3) ────────────────────────────────────────────

async fn post_abuse_report(
    app: &axum::Router,
    token: Option<&str>,
    body: Value,
) -> StatusCode {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/abuse/report")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    app.clone()
        .oneshot(builder.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn abuse_report_succeeds_for_authenticated_reporter() {
    let app = routes::router().with_state(test_state().await);
    let (_did, token) = register_and_get_token(&app).await;
    let status = post_abuse_report(
        &app,
        Some(&token),
        serde_json::json!({"reported_did": "did:plc:spammer", "reason": "spam"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "valid report must be accepted");
}

#[tokio::test]
async fn abuse_report_rejects_unknown_reason() {
    let app = routes::router().with_state(test_state().await);
    let (_did, token) = register_and_get_token(&app).await;
    let status = post_abuse_report(
        &app,
        Some(&token),
        serde_json::json!({"reported_did": "did:plc:x", "reason": "because-i-said-so"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unknown reason must be rejected");
}

#[tokio::test]
async fn abuse_report_requires_auth() {
    let app = routes::router().with_state(test_state().await);
    let status = post_abuse_report(
        &app,
        None,
        serde_json::json!({"reported_did": "did:plc:x", "reason": "spam"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "report must require a session token");
}

// ── Server info endpoint tests ────────────────────────────────────────────────

#[tokio::test]
async fn info_returns_server_name() {
    let app = routes::router().with_state(test_state().await);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["server_name"].is_string());
    assert_eq!(body["privacy_policy_url"], Value::Null);
}

// ── Provisioning mailbox + device-link tests ─────────────────────────────────

/// Create a provisioning session and return its id.
async fn create_provisioning_session(app: &axum::Router) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/provisioning/sessions")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    body["session_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn provisioning_session_put_get_handshake() {
    let app = routes::router().with_state(test_state().await);
    let session_id = create_provisioning_session(&app).await;

    // GET before any PUT → 404.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/provisioning/{session_id}/handshake"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // PUT the handshake slot.
    let put_body = serde_json::json!({ "ciphertext": BASE64_STANDARD.encode([7u8; 33]) });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/provisioning/{session_id}/handshake"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // GET it back.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/provisioning/{session_id}/handshake"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        BASE64_STANDARD.decode(body["ciphertext"].as_str().unwrap()).unwrap(),
        vec![7u8; 33]
    );
}

#[tokio::test]
async fn provisioning_unknown_slot_returns_400() {
    let app = routes::router().with_state(test_state().await);
    let session_id = create_provisioning_session(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/provisioning/{session_id}/not-a-slot"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn provisioning_put_missing_session_returns_404() {
    let app = routes::router().with_state(test_state().await);
    let put_body = serde_json::json!({ "ciphertext": BASE64_STANDARD.encode([1u8; 4]) });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/provisioning/does-not-exist/handshake")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn provisioning_oversize_slot_returns_400() {
    let app = routes::router().with_state(test_state().await);
    let session_id = create_provisioning_session(&app).await;
    // 17 KiB > MAX_SLOT_BYTES (16 KiB).
    let put_body = serde_json::json!({ "ciphertext": BASE64_STANDARD.encode(vec![0u8; 17 * 1024]) });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/provisioning/{session_id}/bundle"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn link_device_bad_base64_rotation_key_returns_400() {
    let app = routes::router().with_state(test_state().await);
    let body = serde_json::json!({
        "did": "did:plc:linktest00000000001",
        "new_device_id": 2,
        "new_identity_key": BASE64_STANDARD.encode([1u8; 33]),
        "new_registration_id": 5,
        "nonce": "n",
        "rotation_key_signature": BASE64_STANDARD.encode([0u8; 8]),
        "rotation_key": "!!!not base64!!!",
        "signed_prekey": { "id": 1, "public_key": BASE64_STANDARD.encode([2u8; 32]), "signature": BASE64_STANDARD.encode([3u8; 64]) },
        "one_time_prekeys": [],
        "kyber_prekey": { "id": 1, "public_key": BASE64_STANDARD.encode([5u8; 32]), "signature": BASE64_STANDARD.encode([6u8; 64]) },
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/devices/link")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn link_device_bad_signature_returns_401() {
    use p256::ecdsa::{signature::Signer, Signature, SigningKey};

    let app = routes::router().with_state(test_state().await);

    // A real P-256 rotation key, but a signature over the WRONG payload, so
    // verification fails before any PLC lookup is attempted.
    let signing_key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let rotation_key = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let sig: Signature = signing_key.sign(b"this is not the link payload");

    let body = serde_json::json!({
        "did": "did:plc:linktest00000000002",
        "new_device_id": 2,
        "new_identity_key": BASE64_STANDARD.encode([1u8; 33]),
        "new_registration_id": 5,
        "nonce": "n",
        "rotation_key_signature": BASE64_STANDARD.encode(sig.to_der().as_bytes()),
        "rotation_key": BASE64_STANDARD.encode(&rotation_key),
        "signed_prekey": { "id": 1, "public_key": BASE64_STANDARD.encode([2u8; 32]), "signature": BASE64_STANDARD.encode([3u8; 64]) },
        "one_time_prekeys": [],
        "kyber_prekey": { "id": 1, "public_key": BASE64_STANDARD.encode([5u8; 32]), "signature": BASE64_STANDARD.encode([6u8; 64]) },
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/devices/link")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── OAuth Project login (docs/25) ────────────────────────────────────────────

/// A test AppState with one registered OAuth login client and (by default) a
/// zero poll-interval so device-grant happy-path tests don't hit `slow_down`.
async fn oauth_test_state() -> AppState {
    let mut state = test_state().await;
    state.config.projects_json = r#"[
        {"name":"Proj","url":"https://proj.test","description":"p",
         "client_id":"cid-test","redirect_uris":["https://proj.test/cb"],"official":true}
    ]"#
    .to_string();
    state.config.oauth_device_poll_interval_secs = 0;
    state
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post_form(app: &axum::Router, uri: &str, body: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[tokio::test]
async fn oauth_authorization_code_flow_end_to_end() {
    let app = routes::router().with_state(oauth_test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();
    let did = reg["did"].as_str().unwrap().to_string();

    let verifier = "test-verifier-0123456789-abcdefghijklmnopqrstuvwx";
    let challenge = pkce_challenge(verifier);

    // App mints the auth code post-consent (session-auth).
    let body = serde_json::json!({
        "client_id": "cid-test",
        "redirect_uri": "https://proj.test/cb",
        "code_challenge": challenge,
        "code_challenge_method": "S256",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/oauth/authorize-code")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let code = body_json(resp).await["code"].as_str().unwrap().to_string();

    // Project exchanges the code (+ verifier) for an access token.
    let form = format!(
        "grant_type=authorization_code&code={code}&redirect_uri=https%3A%2F%2Fproj.test%2Fcb&code_verifier={verifier}&client_id=cid-test"
    );
    let resp = post_form(&app, "/v1/oauth/token", &form).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let tok = body_json(resp).await;
    assert_eq!(tok["token_type"], "Bearer");
    assert!(tok["auth_time"].as_i64().is_some());
    let access = tok["access_token"].as_str().unwrap().to_string();

    // The access token is a project token: verify resolves the DID.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/project-token/verify?token={access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["did"], did);
    assert_eq!(v["project_url"], "https://proj.test");

    // Single-use: replaying the code fails.
    let resp = post_form(&app, "/v1/oauth/token", &form).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(resp).await["error"], "invalid_grant");
}

#[tokio::test]
async fn oauth_authorization_code_wrong_verifier_rejected() {
    let app = routes::router().with_state(oauth_test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let body = serde_json::json!({
        "client_id": "cid-test",
        "redirect_uri": "https://proj.test/cb",
        "code_challenge": pkce_challenge("the-real-verifier-aaaaaaaaaaaaaaaaaaaaaaaa"),
        "code_challenge_method": "S256",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/oauth/authorize-code")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let code = body_json(resp).await["code"].as_str().unwrap().to_string();

    let form = format!(
        "grant_type=authorization_code&code={code}&redirect_uri=https%3A%2F%2Fproj.test%2Fcb&code_verifier=WRONG-verifier-bbbbbbbbbbbbbbbbbbbbbbbb&client_id=cid-test"
    );
    let resp = post_form(&app, "/v1/oauth/token", &form).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(resp).await["error"], "invalid_grant");
}

#[tokio::test]
async fn oauth_authorize_code_rejects_unregistered_redirect_uri() {
    let app = routes::router().with_state(oauth_test_state().await);
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();

    let body = serde_json::json!({
        "client_id": "cid-test",
        "redirect_uri": "https://evil.test/cb",
        "code_challenge": pkce_challenge("v-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "code_challenge_method": "S256",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/oauth/authorize-code")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oauth_device_flow_end_to_end() {
    let app = routes::router().with_state(oauth_test_state().await);

    // Project starts a device grant.
    let resp = post_form(&app, "/v1/oauth/device_authorization", "client_id=cid-test").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let da = body_json(resp).await;
    let device_code = da["device_code"].as_str().unwrap().to_string();
    let user_code = da["user_code"].as_str().unwrap().to_string();
    assert!(da["verification_uri_complete"].as_str().unwrap().contains("/authorize?"));

    let poll = format!("grant_type={DEVICE_GRANT}&device_code={device_code}&client_id=cid-test");

    // Before approval → authorization_pending.
    let resp = post_form(&app, "/v1/oauth/token", &poll).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(resp).await["error"], "authorization_pending");

    // Phone consents and approves (session-auth).
    let reg = register_dummy(&app).await;
    let token = reg["session_token"].as_str().unwrap().to_string();
    let did = reg["did"].as_str().unwrap().to_string();
    let approve = serde_json::json!({ "user_code": user_code, "client_id": "cid-test" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/oauth/device/approve")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&approve).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // After approval → access token; verify resolves the approver's DID.
    let resp = post_form(&app, "/v1/oauth/token", &poll).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let access = body_json(resp).await["access_token"].as_str().unwrap().to_string();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/project-token/verify?token={access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["did"], did);

    // Single-use: a second poll after collection fails.
    let resp = post_form(&app, "/v1/oauth/token", &poll).await;
    assert_eq!(body_json(resp).await["error"], "invalid_grant");
}

#[tokio::test]
async fn oauth_device_flow_slow_down_on_rapid_poll() {
    let mut state = oauth_test_state().await;
    state.config.oauth_device_poll_interval_secs = 3600; // force slow_down on the 2nd poll
    let app = routes::router().with_state(state);

    let resp = post_form(&app, "/v1/oauth/device_authorization", "client_id=cid-test").await;
    let device_code = body_json(resp).await["device_code"].as_str().unwrap().to_string();
    let poll = format!("grant_type={DEVICE_GRANT}&device_code={device_code}&client_id=cid-test");

    let resp = post_form(&app, "/v1/oauth/token", &poll).await;
    assert_eq!(body_json(resp).await["error"], "authorization_pending");
    let resp = post_form(&app, "/v1/oauth/token", &poll).await;
    assert_eq!(body_json(resp).await["error"], "slow_down");
}

#[tokio::test]
async fn oauth_device_authorization_unknown_client_rejected() {
    let app = routes::router().with_state(oauth_test_state().await);
    let resp = post_form(&app, "/v1/oauth/device_authorization", "client_id=nope").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(resp).await["error"], "invalid_client");
}

#[tokio::test]
async fn oauth_token_unsupported_grant_type() {
    let app = routes::router().with_state(oauth_test_state().await);
    let resp = post_form(&app, "/v1/oauth/token", "grant_type=password&username=x").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(resp).await["error"], "unsupported_grant_type");
}
