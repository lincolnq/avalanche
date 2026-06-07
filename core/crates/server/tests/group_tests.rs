//! End-to-end tests for action-bound group endpoints. Exercises the whole
//! flow — create-group → issue-credential → present → submit-change — against
//! a real Postgres via the same `tower::oneshot` harness used by
//! `http_tests.rs`. Requires `TEST_DATABASE_URL`.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::prelude::*;
use crypto::groups::{
    did_to_uuid, AuthCredentialWithPniZkcResponse, GroupKey, RedemptionTime, ServerSecretParams,
};
use crypto::sender_cert::SenderCertChain;
use libsignal_core::{Aci, Pni};
// `B64` is what every group endpoint speaks (URL-safe, no padding). `BASE64_STANDARD`
// stays in scope for the registration body helpers below, which talk to the
// non-group endpoints that still use standard base64.
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use http_body_util::BodyExt;
use libsignal_protocol as signal;
use rand::TryRngCore as _;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tower::ServiceExt;

use server::{config::Config, db, routes, state::AppState};

static SETUP: OnceCell<()> = OnceCell::const_new();

async fn ensure_setup(url: &str) {
    SETUP
        .get_or_init(|| async {
            unsafe { std::env::set_var("ACTNET_DISABLE_IP_RATE_LIMITS", "1") };
            let pool = PgPool::connect(url).await.expect("connect");
            server::migrate::run(&pool).await.expect("migrate");
        })
        .await;
}

async fn test_state() -> AppState {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run server tests");
    ensure_setup(&url).await;
    let pool = PgPool::connect(&url).await.expect("connect");
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
        privacy_policy_url: None,
    };
    let mut conn = pool.acquire().await.expect("acquire");
    let bytes = db::zkgroup_params::load_or_init(
        &mut conn,
        db::zkgroup_params::CURRENT_VERSION,
        || ServerSecretParams::generate().to_bytes(),
    )
    .await
    .expect("load zkgroup params");
    drop(conn);
    let zkgroup_secret = ServerSecretParams::from_bytes(&bytes).expect("decode params");
    let sender_cert_chain = SenderCertChain::generate().expect("sender cert chain");
    AppState::new(pool, config, zkgroup_secret, sender_cert_chain)
}

/// Register a fresh bot account and return its DID + keypair.
async fn register(app: &axum::Router) -> (String, signal::IdentityKeyPair) {
    let keypair = signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err());
    let body = json!({
        "identity_key":    BASE64_STANDARD.encode(keypair.identity_key().serialize()),
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

async fn get_session_token(
    app: &axum::Router,
    did: &str,
    keypair: &signal::IdentityKeyPair,
) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/challenge")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "did": did, "device_id": 1 })).unwrap(),
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
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "did": did, "device_id": 1, "nonce": nonce,
                        "signature": BASE64_URL_SAFE_NO_PAD.encode(sig),
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&bytes).unwrap()["session_token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Day-aligned redemption time the server will require.
fn today_secs() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    now - (now % 86_400)
}

fn today() -> RedemptionTime {
    RedemptionTime::from_epoch_seconds(today_secs())
}

/// One participant: their account on the server + a credential for the
/// group they're acting in.
struct Member {
    did: String,
    session_token: String,
    credential: crypto::groups::AuthCredentialWithPniZkc,
}

async fn make_member(app: &axum::Router) -> Member {
    let (did, kp) = register(app).await;
    let session_token = get_session_token(app, &did, &kp).await;
    // Drop the credential issuance through the real endpoint — that path
    // also exercises rate limiting + DID match validation.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups/credentials")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session_token}"))
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "did": did,
                        "redemption_time": today_secs(),
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "credential issuance");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let response_bytes = B64.decode(body["response"].as_str().unwrap()).unwrap();
    let response: AuthCredentialWithPniZkcResponse = bincode::deserialize(&response_bytes).unwrap();

    // Client fetches public params (we do it via the server-params endpoint
    // because the test wants to exercise that path too).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/groups/server-params")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let server_public_b64 = serde_json::from_slice::<Value>(&bytes).unwrap()["params"]
        .as_str()
        .unwrap()
        .to_string();
    let server_public = crypto::groups::ServerPublicParams::from_bytes(
        &B64.decode(server_public_b64).unwrap(),
    )
    .unwrap();
    let uuid = did_to_uuid(&did);
    let credential = response
        .receive(Aci::from(uuid), Pni::from(uuid), today(), server_public.zkgroup())
        .unwrap();
    Member {
        did,
        session_token,
        credential,
    }
}

/// Helper: present a credential and base64 the bytes for the `X-Group-Auth`
/// header.
fn presentation_header(
    public_params: &crypto::groups::ServerPublicParams,
    group: &GroupKey,
    cred: &crypto::groups::AuthCredentialWithPniZkc,
) -> String {
    let mut r = [0u8; zkcredential::RANDOMNESS_LEN];
    rand::rngs::OsRng.try_fill_bytes(&mut r).unwrap();
    let presentation = cred.present(public_params.zkgroup(), group.zkgroup_secret(), r);
    B64.encode(bincode::serialize(&presentation).unwrap())
}

/// `encrypted_member_id` for `did` under `group_key`, base64-encoded.
fn emi_for(group: &GroupKey, did: &str) -> String {
    let r = [0u8; zkcredential::RANDOMNESS_LEN];
    let server = ServerSecretParams::generate();
    let public = server.public_params();
    let uuid = did_to_uuid(did);
    let response = AuthCredentialWithPniZkcResponse::issue_credential(
        Aci::from(uuid),
        Pni::from(uuid),
        today(),
        server.zkgroup(),
        r,
    );
    let cred = response
        .receive(Aci::from(uuid), Pni::from(uuid), today(), public.zkgroup())
        .unwrap();
    let presentation = cred.present(public.zkgroup(), group.zkgroup_secret(), r);
    B64.encode(zkgroup::serialize(&presentation.aci_ciphertext()))
}

fn default_policy() -> Value {
    json!({
        "invite_members_role": 1,
        "remove_members_role": 1,
        "modify_title_role": 1,
        "modify_description_role": 1,
        "modify_expiry_role": 1,
        "join_policy": 0,
        "invite_link_password": null,
        "announcement_only": false,
    })
}

/// Create a group with `founder` as Admin. Returns the `GroupKey` and the
/// base64 group_id used in URLs.
async fn create_group_for(app: &axum::Router, founder: &Member) -> (GroupKey, String) {
    let group_key = GroupKey::generate();
    let group_id = group_key.group_id().0;
    let founder_emi = emi_for(&group_key, &founder.did);
    let founder_pseudonym = B64.encode([0xAA; 32]);
    let body = json!({
        "group_public_params": B64.encode(group_key.public_params().to_bytes()),
        "encrypted_state": B64.encode(group_key.encrypt_state(b"initial state v0")),
        "founder_encrypted_member_id": founder_emi,
        "founder_group_push_pseudonym": founder_pseudonym,
        "policy": default_policy(),
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", founder.session_token))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "create_group");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["revision"], 0);
    // Group endpoints speak URL-safe-no-pad everywhere; the response body
    // and the URL path are encoded the same way.
    let group_id_b64 = B64.encode(group_id);
    assert_eq!(body["group_id"], group_id_b64);
    (group_key, group_id_b64)
}

// ── tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_group_then_fetch_state() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &founder.credential);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/groups/{group_id_b64}"))
                .header("x-group-auth", &pres)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["revision"], 0);
    let state_b64 = body["encrypted_state"].as_str().unwrap();
    let state_bytes = B64.decode(state_b64).unwrap();
    let plaintext = group_key.decrypt_state(&state_bytes).unwrap();
    assert_eq!(plaintext, b"initial state v0");
}

#[tokio::test]
async fn create_group_duplicate_returns_conflict() {
    let state = test_state().await;
    let app = routes::router().with_state(state);
    let founder = make_member(&app).await;
    // Use the same GroupKey for both attempts so they derive the same
    // group_id; second create must 409.
    let group_key = GroupKey::generate();
    let founder_emi = emi_for(&group_key, &founder.did);
    let body = json!({
        "group_public_params": B64.encode(group_key.public_params().to_bytes()),
        "encrypted_state": B64.encode(group_key.encrypt_state(b"v0")),
        "founder_encrypted_member_id": founder_emi,
        "founder_group_push_pseudonym": B64.encode([0xAA; 32]),
        "policy": default_policy(),
    });
    let make_request = || {
        let body = body.clone();
        let token = founder.session_token.clone();
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/groups")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let resp1 = make_request().await;
    assert_eq!(resp1.status(), StatusCode::OK);
    let resp2 = make_request().await;
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn get_group_without_membership_returns_404() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let outsider = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    // Outsider has a valid daily credential but isn't in member_credentials.
    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &outsider.credential);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/groups/{group_id_b64}"))
                .header("x-group-auth", &pres)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // §3.4 + information-flow leak prevention: same 404 as "no such group".
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn submit_change_invite_then_promote_full_roundtrip() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let invitee = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let invitee_emi = emi_for(&group_key, &invitee.did);
    let founder_pres = presentation_header(&public_params, &group_key, &founder.credential);

    // Founder invites the invitee.
    let invite_body = json!({
        "revision": 1,
        "new_encrypted_state": B64.encode(group_key.encrypt_state(b"state after invite")),
        "actions": {
            "invite_members": [{ "encrypted_member_id": invitee_emi, "role": 0 }],
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &founder_pres)
                .body(Body::from(serde_json::to_vec(&invite_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "invite");

    // Invitee promotes themselves into membership.
    let invitee_pres = presentation_header(&public_params, &group_key, &invitee.credential);
    let promote_body = json!({
        "revision": 2,
        "new_encrypted_state": B64.encode(group_key.encrypt_state(b"state after promote")),
        "actions": {
            "promote_pending_members": {
                "encrypted_profile_key": B64.encode([0xCC; 32]),
                "group_push_pseudonym": B64.encode([0xDD; 32]),
            }
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &invitee_pres)
                .body(Body::from(serde_json::to_vec(&promote_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "promote");

    // Invitee can now fetch the group state.
    let invitee_pres2 = presentation_header(&public_params, &group_key, &invitee.credential);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/groups/{group_id_b64}"))
                .header("x-group-auth", &invitee_pres2)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "post-promote fetch");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["revision"], 2);
}

#[tokio::test]
async fn submit_change_stale_revision_returns_409() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let other = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &founder.credential);

    // Submit revision=2 when current is 0. Server expects revision=1.
    let body = json!({
        "revision": 2,
        "new_encrypted_state": B64.encode([0u8; 32]),
        "actions": {
            "invite_members": [{ "encrypted_member_id": emi_for(&group_key, &other.did), "role": 0 }],
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &pres)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn submit_change_non_member_returns_401() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let outsider = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &outsider.credential);

    // Outsider tries to invite themselves with an admin-class action.
    let body = json!({
        "revision": 1,
        "new_encrypted_state": B64.encode([0u8; 32]),
        "actions": {
            "invite_members": [{ "encrypted_member_id": emi_for(&group_key, &outsider.did), "role": 0 }],
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &pres)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn submit_change_self_action_with_admin_action_returns_400() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &founder.credential);

    let body = json!({
        "revision": 1,
        "new_encrypted_state": B64.encode([0u8; 32]),
        "actions": {
            "invite_members": [{ "encrypted_member_id": emi_for(&group_key, "did:plc:other"), "role": 0 }],
            "decline_invite": B64.encode([0u8; 64]),
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &pres)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_changes_returns_history() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let other = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();

    // Submit one change so there's something in the history.
    let pres = presentation_header(&public_params, &group_key, &founder.credential);
    let body = json!({
        "revision": 1,
        "new_encrypted_state": B64.encode([1u8; 32]),
        "actions": {
            "invite_members": [{ "encrypted_member_id": emi_for(&group_key, &other.did), "role": 0 }],
        }
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/changes"))
                .header("content-type", "application/json")
                .header("x-group-auth", &pres)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let pres2 = presentation_header(&public_params, &group_key, &founder.credential);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/groups/{group_id_b64}/changes?from_revision=0"))
                .header("x-group-auth", &pres2)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["current_revision"], 1);
    let changes = body["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["revision"], 0);
}

#[tokio::test]
async fn issue_credential_for_other_did_returns_401() {
    let state = test_state().await;
    let app = routes::router().with_state(state);
    let alice = make_member(&app).await;
    let bob = make_member(&app).await;

    // Alice's session bearer + Bob's DID. Server must reject.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups/credentials")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", alice.session_token))
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "did": bob.did,
                        "redemption_time": today_secs(),
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn push_binding_rotates_pseudonym_for_member() {
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());
    let founder = make_member(&app).await;
    let (group_key, group_id_b64) = create_group_for(&app, &founder).await;

    let public_params = state.zkgroup_secret.public_params();
    let pres = presentation_header(&public_params, &group_key, &founder.credential);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id_b64}/push_binding"))
                .header("content-type", "application/json")
                .header("x-group-auth", &pres)
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "new_group_push_pseudonym": B64.encode([0xEE; 32])
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}
