//! Database layer tests using the transaction-rollback pattern.
//!
//! Each test begins a transaction, runs its assertions, then the transaction
//! is dropped (rolled back) on exit. This means:
//! - Tests are fast (no per-test DB setup/teardown).
//! - Tests are isolated (writes never commit, so tests don't interfere).
//! - Tests need a running Postgres with the schema already applied.
//!
//! Set `TEST_DATABASE_URL` to point at a test Postgres instance.
//! Schema migrations are applied automatically on first connect via the same
//! embedded migrator the server binary uses.

use sqlx::{PgPool, Row};
use tokio::sync::OnceCell;

static MIGRATED: OnceCell<()> = OnceCell::const_new();

/// Connect to the test database and ensure migrations are applied. Panics if
/// TEST_DATABASE_URL is not set. Each call returns a fresh pool owned by the
/// caller's runtime; migrations are applied at most once across all tests.
async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run server tests");
    let pool = PgPool::connect(&url).await.expect("failed to connect to test database");
    MIGRATED
        .get_or_init(|| async {
            server::migrate::run(&pool).await.expect("failed to apply test migrations");
        })
        .await;
    pool
}

/// Begin a transaction that will be rolled back when dropped.
/// Returns a connection usable as `&mut PgConnection` via `&mut *tx`.
async fn begin_tx(pool: &PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
    pool.begin().await.expect("failed to begin transaction")
}

// ── Account tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_account_returns_id() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let id = server::db::accounts::create(&mut *tx, "did:plc:testaccount1", None, false).await.unwrap();
    assert!(id > 0);
}

#[tokio::test]
async fn find_account_by_did() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:findme123456789012";
    let id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    let found = server::db::accounts::find_by_did(&mut *tx, did).await.unwrap();
    assert!(found.is_some());
    let account = found.unwrap();
    assert_eq!(account.id, id);
    assert_eq!(account.did, did);
    assert_eq!(account.display_name, None);
    assert!(!account.is_bot);
}

#[tokio::test]
async fn find_account_nonexistent_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let found = server::db::accounts::find_by_did(&mut *tx, "did:plc:doesnotexist0000").await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn create_bot_account_with_display_name() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:bottest0000000001";
    let id = server::db::accounts::create(&mut *tx, did, Some("Actbot"), true).await.unwrap();
    assert!(id > 0);

    let account = server::db::accounts::find_by_did(&mut *tx, did).await.unwrap().unwrap();
    assert_eq!(account.display_name.as_deref(), Some("Actbot"));
    assert!(account.is_bot);
}

// ── Device tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_and_find_device() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:devicetest00000001", None, false).await.unwrap();
    let identity_key = vec![1u8; 33];
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &identity_key, 42).await.unwrap();
    assert!(device_pk > 0);

    let device = server::db::devices::find(&mut *tx, account_id, 1).await.unwrap().unwrap();
    assert_eq!(device.id, device_pk);
    assert_eq!(device.device_id, 1);
    assert_eq!(device.identity_key, identity_key);
    assert_eq!(device.registration_id, 42);
}

#[tokio::test]
async fn find_device_by_did() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:devbydidtest000001";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();
    let identity_key = vec![2u8; 33];
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &identity_key, 99).await.unwrap();

    let device = server::db::devices::find_by_did(&mut *tx, did, 1).await.unwrap().unwrap();
    assert_eq!(device.id, device_pk);
}

#[tokio::test]
async fn list_devices_by_did() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:listdevtest0000001";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();
    server::db::devices::create(&mut *tx, account_id, 1, &[1u8; 33], 10).await.unwrap();
    server::db::devices::create(&mut *tx, account_id, 2, &[2u8; 33], 20).await.unwrap();

    let devices = server::db::devices::list_by_did(&mut *tx, did).await.unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].device_id, 1);
    assert_eq!(devices[1].device_id, 2);
}

// ── Session token tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn create_and_validate_session_token() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:sessiontest000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[3u8; 33], 1).await.unwrap();

    let token = "test-token-abc123";
    let _expires = server::db::sessions::create(&mut *tx, token, device_pk, 3600).await.unwrap();

    let validated = server::db::sessions::validate(&mut *tx, token).await.unwrap();
    assert_eq!(validated, Some(device_pk));
}

#[tokio::test]
async fn validate_invalid_token_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let validated = server::db::sessions::validate(&mut *tx, "nonexistent-token").await.unwrap();
    assert_eq!(validated, None);
}

#[tokio::test]
async fn expired_token_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:expiredtoken00001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[4u8; 33], 1).await.unwrap();

    // Create a token with 0-second lifetime (already expired).
    let token = "expired-token-xyz";
    server::db::sessions::create(&mut *tx, token, device_pk, 0).await.unwrap();

    let validated = server::db::sessions::validate(&mut *tx, token).await.unwrap();
    assert_eq!(validated, None);
}

// ── DID document tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn upsert_and_find_did_document() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:diddoctest00000001";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    let doc = serde_json::json!({
        "id": did,
        "verificationMethod": []
    });
    server::db::did::upsert_document(&mut *tx, account_id, &doc).await.unwrap();

    let found = server::db::did::find_by_did(&mut *tx, did).await.unwrap().unwrap();
    assert_eq!(found["id"], did);
}

// ── Prekey tests ─────────────────────────────────────────────────────────────

async fn setup_device(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, did: &str) -> i64 {
    let account_id = server::db::accounts::create(&mut **tx, did, None, false).await.unwrap();
    server::db::devices::create(&mut **tx, account_id, 1, &[5u8; 33], 100).await.unwrap()
}

#[tokio::test]
async fn upload_and_count_prekeys() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:prekeytest00000001").await;

    // Upload signed prekey.
    server::db::prekeys::upsert_signed(&mut *tx, device_pk, 1, &[10u8; 32], &[11u8; 64]).await.unwrap();

    // Upload one-time prekeys.
    let otpks = vec![
        (1, vec![20u8; 32]),
        (2, vec![21u8; 32]),
        (3, vec![22u8; 32]),
    ];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();

    let count = server::db::prekeys::one_time_count(&mut *tx, device_pk).await.unwrap();
    assert_eq!(count, 3);

    // Upload Kyber prekey.
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 1, &[30u8; 32], &[31u8; 64]).await.unwrap();

    let kyber_count = server::db::prekeys::kyber_count(&mut *tx, device_pk).await.unwrap();
    assert_eq!(kyber_count, 1);
}

#[tokio::test]
async fn fetch_bundle_consumes_one_time_prekey() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:bundletest00000001").await;

    server::db::prekeys::upsert_signed(&mut *tx, device_pk, 1, &[10u8; 32], &[11u8; 64]).await.unwrap();
    let otpks = vec![(1, vec![20u8; 32]), (2, vec![21u8; 32])];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 1, &[30u8; 32], &[31u8; 64]).await.unwrap();

    // First fetch should consume one OTP key.
    let bundle = server::db::prekeys::fetch_bundle(&mut *tx, device_pk).await.unwrap().unwrap();
    assert!(bundle.one_time_prekey.is_some());
    assert_eq!(bundle.signed_prekey.id, 1);
    assert_eq!(bundle.kyber_prekey.id, 1);

    let remaining = server::db::prekeys::one_time_count(&mut *tx, device_pk).await.unwrap();
    assert_eq!(remaining, 1);

    // Second fetch consumes the last one.
    let bundle2 = server::db::prekeys::fetch_bundle(&mut *tx, device_pk).await.unwrap().unwrap();
    assert!(bundle2.one_time_prekey.is_some());

    let remaining = server::db::prekeys::one_time_count(&mut *tx, device_pk).await.unwrap();
    assert_eq!(remaining, 0);

    // Third fetch has no one-time prekey.
    let bundle3 = server::db::prekeys::fetch_bundle(&mut *tx, device_pk).await.unwrap().unwrap();
    assert!(bundle3.one_time_prekey.is_none());
}

// ── One-time Kyber prekey tests ──────────────────────────────────────────────

#[tokio::test]
async fn one_time_kyber_batch_insert_and_count() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:otkybercounttest0001").await;

    let keys = vec![
        (1i32, vec![40u8; 32], vec![41u8; 64]),
        (2i32, vec![42u8; 32], vec![43u8; 64]),
        (3i32, vec![44u8; 32], vec![45u8; 64]),
    ];
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &keys)
        .await
        .unwrap();

    let count = server::db::prekeys::one_time_kyber_count(&mut *tx, device_pk)
        .await
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn fetch_bundle_consumes_one_time_kyber() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:otkyberbundletest001").await;

    server::db::prekeys::upsert_signed(&mut *tx, device_pk, 1, &[10u8; 32], &[11u8; 64])
        .await
        .unwrap();
    // Last-resort Kyber prekey (required for fallback).
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 1, &[30u8; 32], &[31u8; 64])
        .await
        .unwrap();
    // One-time Kyber pool with 2 keys.
    let otkpks = vec![
        (1i32, vec![40u8; 32], vec![41u8; 64]),
        (2i32, vec![42u8; 32], vec![43u8; 64]),
    ];
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &otkpks)
        .await
        .unwrap();

    // First fetch should consume one one-time Kyber key.
    let bundle = server::db::prekeys::fetch_bundle(&mut *tx, device_pk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.kyber_prekey.id, 1);

    let remaining = server::db::prekeys::one_time_kyber_count(&mut *tx, device_pk)
        .await
        .unwrap();
    assert_eq!(remaining, 1);

    // Second fetch consumes the last one-time Kyber key.
    let bundle2 = server::db::prekeys::fetch_bundle(&mut *tx, device_pk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle2.kyber_prekey.id, 2);

    let remaining = server::db::prekeys::one_time_kyber_count(&mut *tx, device_pk)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn fetch_bundle_falls_back_to_last_resort_kyber() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:kyberfalbacktest001").await;

    server::db::prekeys::upsert_signed(&mut *tx, device_pk, 1, &[10u8; 32], &[11u8; 64])
        .await
        .unwrap();
    // Only last-resort Kyber — no one-time Kyber pool.
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 99, &[30u8; 32], &[31u8; 64])
        .await
        .unwrap();

    let bundle = server::db::prekeys::fetch_bundle(&mut *tx, device_pk)
        .await
        .unwrap()
        .unwrap();

    // Bundle should still contain the last-resort Kyber key.
    assert_eq!(bundle.kyber_prekey.id, 99);

    // One-time Kyber pool remains empty.
    let count = server::db::prekeys::one_time_kyber_count(&mut *tx, device_pk)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ── Message queue tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_and_fetch_messages() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:msgtest000000000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[6u8; 33], 1).await.unwrap();

    let _msg1 = server::db::messages::enqueue(&mut *tx, device_pk, Some(account_id), None, b"cipher1", 1, 86400).await.unwrap();
    let _msg2 = server::db::messages::enqueue(&mut *tx, device_pk, Some(account_id), None, b"cipher2", 1, 86400).await.unwrap();

    let queued = server::db::messages::fetch_for_device(&mut *tx, device_pk).await.unwrap();
    assert_eq!(queued.len(), 2);
    // Messages are ordered by enqueued_at ASC; verify content rather than IDs
    // since sequence counters are shared with committed e2e test data.
    assert_eq!(queued[0].ciphertext, b"cipher1");
    assert_eq!(queued[1].ciphertext, b"cipher2");
    assert!(queued[0].id < queued[1].id);
}

#[tokio::test]
async fn acknowledge_deletes_messages() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:acktest0000000000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[7u8; 33], 1).await.unwrap();

    let msg1 = server::db::messages::enqueue(&mut *tx, device_pk, None, None, b"c1", 1, 86400).await.unwrap();
    let msg2 = server::db::messages::enqueue(&mut *tx, device_pk, None, None, b"c2", 1, 86400).await.unwrap();

    // Acknowledge only the first message.
    let deleted = server::db::messages::acknowledge(&mut *tx, device_pk, &[msg1]).await.unwrap();
    assert_eq!(deleted, 1);

    let remaining = server::db::messages::fetch_for_device(&mut *tx, device_pk).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, msg2);
}

#[tokio::test]
async fn acknowledge_scoped_to_device() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:scopetest00000000001", None, false).await.unwrap();
    let device1 = server::db::devices::create(&mut *tx, account_id, 1, &[8u8; 33], 1).await.unwrap();
    let device2 = server::db::devices::create(&mut *tx, account_id, 2, &[9u8; 33], 2).await.unwrap();

    let msg1 = server::db::messages::enqueue(&mut *tx, device1, None, None, b"for-dev1", 1, 86400).await.unwrap();

    // device2 trying to ack device1's message should have no effect.
    let deleted = server::db::messages::acknowledge(&mut *tx, device2, &[msg1]).await.unwrap();
    assert_eq!(deleted, 0);

    // device1 can ack its own message.
    let deleted = server::db::messages::acknowledge(&mut *tx, device1, &[msg1]).await.unwrap();
    assert_eq!(deleted, 1);
}

#[tokio::test]
async fn message_without_sender() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:nosender000000000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[10u8; 33], 1).await.unwrap();

    // sender_account_id = None (sealed sender future mode).
    let msg_id = server::db::messages::enqueue(&mut *tx, device_pk, None, None, b"sealed", 1, 86400).await.unwrap();
    assert!(msg_id > 0);

    let queued = server::db::messages::fetch_for_device(&mut *tx, device_pk).await.unwrap();
    assert_eq!(queued.len(), 1);
}

// ── Message expiry tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn send_with_custom_expiry_sets_expires_at() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:expirytest000000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[20u8; 33], 1).await.unwrap();

    // Enqueue with a 600-second expiry.
    let expiry_secs: i64 = 600;
    let _msg_id = server::db::messages::enqueue(
        &mut *tx, device_pk, Some(account_id), None, b"cipher", 1, expiry_secs,
    ).await.unwrap();

    // Read back expires_at and verify it's approximately now + 600s.
    let row = sqlx::query("SELECT expires_at FROM message_queue WHERE recipient_device_pk = $1")
        .bind(device_pk)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let expires_at: time::OffsetDateTime = row.get("expires_at");
    let now = time::OffsetDateTime::now_utc();
    let diff = (expires_at - now).whole_seconds();
    // Allow a 5-second tolerance for test execution time.
    assert!(diff >= expiry_secs - 5 && diff <= expiry_secs + 5,
        "expires_at should be ~{expiry_secs}s from now, got diff={diff}s");
}

#[tokio::test]
async fn send_expiry_is_clamped_to_min() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:expiryclamped00001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[21u8; 33], 1).await.unwrap();

    // The clamping is enforced at the route layer, not in db::messages::enqueue.
    // Here we test the clamping logic directly with a 60s requested expiry and
    // a 300s minimum — simulating what the route handler does before calling enqueue.
    let requested: i64 = 60;
    let min_secs: i64 = 300;
    let max_secs: i64 = 2_592_000;
    let clamped = requested.clamp(min_secs, max_secs);
    assert_eq!(clamped, min_secs, "expiry below min should be clamped up to min");

    // Verify enqueue with the clamped value stores the correct expires_at.
    let _msg_id = server::db::messages::enqueue(
        &mut *tx, device_pk, Some(account_id), None, b"cipher2", 1, clamped,
    ).await.unwrap();

    let row = sqlx::query("SELECT expires_at FROM message_queue WHERE recipient_device_pk = $1")
        .bind(device_pk)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let expires_at: time::OffsetDateTime = row.get("expires_at");
    let now = time::OffsetDateTime::now_utc();
    let diff = (expires_at - now).whole_seconds();
    assert!(diff >= min_secs - 5 && diff <= min_secs + 5,
        "expires_at should reflect clamped min of {min_secs}s, got diff={diff}s");
}

// ── Prekey vacuum tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn prekey_vacuum_sends_notification_when_below_threshold() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumlow000000001").await;

    // Upload 2 one-time EC prekeys and 1 one-time Kyber prekey — both below threshold of 10.
    let otpks = vec![(1, vec![20u8; 32]), (2, vec![21u8; 32])];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    let otkpks = vec![(1i32, vec![30u8; 32], vec![31u8; 64])];
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &otkpks).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("expected a prekey_low notification");
    match msg {
        server::state::WsPush::PrekeyLow { one_time_remaining, kyber_remaining } => {
            assert_eq!(one_time_remaining, 2);
            assert_eq!(kyber_remaining, 1);
        }
        other => panic!("expected PrekeyLow, got {other:?}"),
    }
}

#[tokio::test]
async fn prekey_vacuum_no_notification_when_both_above_threshold() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumhigh00000001").await;

    // Upload 20 one-time EC prekeys and 15 one-time Kyber prekeys — both above threshold of 10.
    let otpks: Vec<(i32, Vec<u8>)> = (1..=20).map(|i| (i, vec![i as u8; 32])).collect();
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    let otkpks: Vec<(i32, Vec<u8>, Vec<u8>)> = (1..=15i32)
        .map(|i| (i, vec![i as u8; 32], vec![31u8; 64]))
        .collect();
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &otkpks).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    assert!(rx_ws.try_recv().is_err(), "both counts above threshold, expected no notification");
}

#[tokio::test]
async fn prekey_vacuum_notifies_when_only_ec_low() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumeclow0000001").await;

    // 2 EC prekeys (below 10), 15 one-time Kyber prekeys (above 10).
    let otpks = vec![(1, vec![20u8; 32]), (2, vec![21u8; 32])];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    let otkpks: Vec<(i32, Vec<u8>, Vec<u8>)> = (1..=15i32)
        .map(|i| (i, vec![i as u8; 32], vec![31u8; 64]))
        .collect();
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &otkpks).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("EC count below threshold should trigger notification");
    match msg {
        server::state::WsPush::PrekeyLow { one_time_remaining, .. } => {
            assert_eq!(one_time_remaining, 2);
        }
        other => panic!("expected PrekeyLow, got {other:?}"),
    }
}

#[tokio::test]
async fn prekey_vacuum_notifies_when_only_kyber_low() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumkyblow000001").await;

    // 20 EC prekeys (above 10), 1 one-time Kyber prekey (below 10).
    let otpks: Vec<(i32, Vec<u8>)> = (1..=20).map(|i| (i, vec![i as u8; 32])).collect();
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    let otkpks = vec![(1i32, vec![30u8; 32], vec![31u8; 64])];
    server::db::prekeys::insert_one_time_kyber_batch(&mut *tx, device_pk, &otkpks).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("Kyber count below threshold should trigger notification");
    match msg {
        server::state::WsPush::PrekeyLow { kyber_remaining, .. } => {
            assert_eq!(kyber_remaining, 1);
        }
        other => panic!("expected PrekeyLow, got {other:?}"),
    }
}

// ── Project token tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn create_and_verify_project_token() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:projtoken000000001";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    let token = "project-token-abc123";
    let _expires = server::db::project_tokens::create(
        &mut *tx, token, account_id, "http://localhost:3001", 3600,
    ).await.unwrap();

    let result = server::db::project_tokens::verify(&mut *tx, token).await.unwrap();
    assert!(result.is_some());
    let (verified_did, project_url) = result.unwrap();
    assert_eq!(verified_did, did);
    assert_eq!(project_url, "http://localhost:3001");
}

#[tokio::test]
async fn verify_invalid_project_token_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let result = server::db::project_tokens::verify(&mut *tx, "nonexistent-project-token").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn expired_project_token_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:expiredproj00000001", None, false).await.unwrap();

    let token = "expired-project-token-xyz";
    server::db::project_tokens::create(
        &mut *tx, token, account_id, "http://localhost:3001", 0,
    ).await.unwrap();

    let result = server::db::project_tokens::verify(&mut *tx, token).await.unwrap();
    assert!(result.is_none());
}

// ── Auth challenge tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn create_and_consume_challenge() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:challengetest000001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[11u8; 33], 1).await.unwrap();

    let nonce = "test-nonce-consume-001";
    server::db::challenges::create(&mut *tx, nonce, device_pk, 300).await.unwrap();

    let result = server::db::challenges::consume(&mut *tx, nonce).await.unwrap();
    assert_eq!(result, Some(device_pk));
}

#[tokio::test]
async fn consume_expired_challenge_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:challengeexpired001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[12u8; 33], 1).await.unwrap();

    let nonce = "test-nonce-expired-001";
    server::db::challenges::create(&mut *tx, nonce, device_pk, 0).await.unwrap();

    let result = server::db::challenges::consume(&mut *tx, nonce).await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn consume_nonexistent_challenge_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let result = server::db::challenges::consume(&mut *tx, "nonce-that-does-not-exist").await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn consume_is_one_time() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:challengeonetime001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &[13u8; 33], 1).await.unwrap();

    let nonce = "test-nonce-onetime-001";
    server::db::challenges::create(&mut *tx, nonce, device_pk, 300).await.unwrap();

    let first = server::db::challenges::consume(&mut *tx, nonce).await.unwrap();
    assert_eq!(first, Some(device_pk));

    let second = server::db::challenges::consume(&mut *tx, nonce).await.unwrap();
    assert_eq!(second, None, "nonce must be single-use");
}

// ── Auth challenge + signature integration test ──────────────────────────────

#[tokio::test]
async fn full_auth_flow_valid_signature_accepted() {
    use base64::prelude::*;
    use libsignal_protocol as signal;
    use rand::{Rng, TryRngCore as _};

    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    // Register a device with a real Ed25519 identity key.
    let keypair = signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err());
    let identity_key_bytes = keypair.identity_key().serialize().to_vec();

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:authflowvalid00001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &identity_key_bytes, 1).await.unwrap();

    // Issue a challenge nonce.
    let nonce_bytes: [u8; 32] = rand::rng().random();
    let nonce = BASE64_URL_SAFE_NO_PAD.encode(nonce_bytes);
    server::db::challenges::create(&mut *tx, &nonce, device_pk, 300).await.unwrap();

    // Client signs the raw nonce bytes.
    let sig = keypair
        .private_key()
        .calculate_signature(&nonce_bytes, &mut rand::rngs::OsRng.unwrap_err())
        .expect("signing failed");

    // Consume the challenge and verify the signature — mirrors the handler logic.
    let challenge_device_pk = server::db::challenges::consume(&mut *tx, &nonce)
        .await.unwrap().expect("challenge should be present");
    assert_eq!(challenge_device_pk, device_pk);

    let device = server::db::devices::find(&mut *tx, account_id, 1)
        .await.unwrap().unwrap();
    let stored_key = signal::IdentityKey::decode(&device.identity_key).expect("decode");
    let valid = stored_key.public_key().verify_signature(&nonce_bytes, &sig);
    assert!(valid, "signature from the correct key should be accepted");
}

#[tokio::test]
async fn full_auth_flow_wrong_signature_rejected() {
    use base64::prelude::*;
    use libsignal_protocol as signal;
    use rand::{Rng, TryRngCore as _};

    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let keypair = signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err());
    let identity_key_bytes = keypair.identity_key().serialize().to_vec();

    let account_id = server::db::accounts::create(&mut *tx, "did:plc:authflowwrong0001", None, false).await.unwrap();
    let device_pk = server::db::devices::create(&mut *tx, account_id, 1, &identity_key_bytes, 1).await.unwrap();

    let nonce_bytes: [u8; 32] = rand::rng().random();
    let nonce = BASE64_URL_SAFE_NO_PAD.encode(nonce_bytes);
    server::db::challenges::create(&mut *tx, &nonce, device_pk, 300).await.unwrap();

    // Sign with a different keypair — wrong key.
    let wrong_keypair = signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err());
    let bad_sig = wrong_keypair
        .private_key()
        .calculate_signature(&nonce_bytes, &mut rand::rngs::OsRng.unwrap_err())
        .expect("signing failed");

    server::db::challenges::consume(&mut *tx, &nonce).await.unwrap();

    let device = server::db::devices::find(&mut *tx, account_id, 1)
        .await.unwrap().unwrap();
    let stored_key = signal::IdentityKey::decode(&device.identity_key).expect("decode");
    let valid = stored_key.public_key().verify_signature(&nonce_bytes, &bad_sig);
    assert!(!valid, "signature from a different key should be rejected");
}

// ── Profile tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn profile_upsert_and_get() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:profiletest0000001";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    let blob = b"opaque-encrypted-profile-bytes";
    server::db::profiles::upsert(&mut *tx, account_id, blob).await.unwrap();

    let got = server::db::profiles::get_by_account_id(&mut *tx, account_id).await.unwrap();
    assert_eq!(got.as_deref(), Some(blob.as_ref()));
}

#[tokio::test]
async fn profile_upsert_overwrites() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:profiletest0000002";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    server::db::profiles::upsert(&mut *tx, account_id, b"v1").await.unwrap();
    server::db::profiles::upsert(&mut *tx, account_id, b"v2").await.unwrap();

    let got = server::db::profiles::get_by_account_id(&mut *tx, account_id).await.unwrap();
    assert_eq!(got.as_deref(), Some(b"v2".as_ref()));
}

#[tokio::test]
async fn profile_get_missing_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let did = "did:plc:profiletest0000003";
    let account_id = server::db::accounts::create(&mut *tx, did, None, false).await.unwrap();

    let got = server::db::profiles::get_by_account_id(&mut *tx, account_id).await.unwrap();
    assert!(got.is_none());
}

// ── Storage service tests (docs/05) ──────────────────────────────────────────

use server::db::storage::{self, PutOutcome};

#[tokio::test]
async fn storage_seq_is_monotonic_per_account() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let a = server::db::accounts::create(&mut *tx, "did:plc:storageseq000000001", None, false).await.unwrap();
    let b = server::db::accounts::create(&mut *tx, "did:plc:storageseq000000002", None, false).await.unwrap();

    // First allocation for an account is 1 (0 stays reserved).
    assert_eq!(storage::alloc_seq(&mut *tx, a).await.unwrap(), 1);
    assert_eq!(storage::alloc_seq(&mut *tx, a).await.unwrap(), 2);
    assert_eq!(storage::alloc_seq(&mut *tx, a).await.unwrap(), 3);
    // Counters are independent per account.
    assert_eq!(storage::alloc_seq(&mut *tx, b).await.unwrap(), 1);
}

#[tokio::test]
async fn storage_put_create_then_pull() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storageput0000001", None, false).await.unwrap();
    let rid = [1u8; 16];

    match storage::put_item(&mut *tx, acc, &rid, 0, false, b"cipher").await.unwrap() {
        PutOutcome::Applied { version, seq } => {
            assert_eq!(version, 1);
            assert_eq!(seq, 1);
        }
        PutOutcome::Conflict { .. } => panic!("create should not conflict"),
    }

    let items = storage::pull(&mut *tx, acc, 0, 500).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].record_id, rid);
    assert_eq!(items[0].ciphertext, b"cipher");
    assert!(!items[0].deleted);
    assert_eq!(items[0].version, 1);
    assert_eq!(items[0].seq, 1);
}

#[tokio::test]
async fn storage_cas_conflict_leaves_row_unchanged() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storagecas0000001", None, false).await.unwrap();
    let rid = [2u8; 16];

    storage::put_item(&mut *tx, acc, &rid, 0, false, b"v1").await.unwrap();

    // A stale expected_version (0, but the row is now version 1) must conflict.
    match storage::put_item(&mut *tx, acc, &rid, 0, false, b"v2").await.unwrap() {
        PutOutcome::Conflict { current_version } => assert_eq!(current_version, 1),
        PutOutcome::Applied { .. } => panic!("stale write should conflict"),
    }

    // The row is untouched: still v1 at version 1.
    let items = storage::pull(&mut *tx, acc, 0, 500).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ciphertext, b"v1");
    assert_eq!(items[0].version, 1);
}

#[tokio::test]
async fn storage_correct_version_update_bumps_version_and_seq() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storageupd0000001", None, false).await.unwrap();
    let rid = [3u8; 16];

    storage::put_item(&mut *tx, acc, &rid, 0, false, b"v1").await.unwrap();

    // Updating with the matching expected_version applies and allocates a new seq.
    match storage::put_item(&mut *tx, acc, &rid, 1, false, b"v2").await.unwrap() {
        PutOutcome::Applied { version, seq } => {
            assert_eq!(version, 2);
            assert_eq!(seq, 2);
        }
        PutOutcome::Conflict { .. } => panic!("matching version should apply"),
    }

    let items = storage::pull(&mut *tx, acc, 0, 500).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ciphertext, b"v2");
    assert_eq!(items[0].version, 2);
}

#[tokio::test]
async fn storage_tombstone_is_recorded() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storagetomb000001", None, false).await.unwrap();
    let rid = [4u8; 16];

    storage::put_item(&mut *tx, acc, &rid, 0, false, b"live").await.unwrap();
    // Delete with the matching version; tombstones carry an empty ciphertext.
    storage::put_item(&mut *tx, acc, &rid, 1, true, b"").await.unwrap();

    let items = storage::pull(&mut *tx, acc, 0, 500).await.unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].deleted);
    assert!(items[0].ciphertext.is_empty());
    assert_eq!(items[0].version, 2);
}

#[tokio::test]
async fn storage_pull_respects_since_and_limit_ordering() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storagepull000001", None, false).await.unwrap();
    // Three independent records → seq 1, 2, 3.
    storage::put_item(&mut *tx, acc, &[10u8; 16], 0, false, b"a").await.unwrap();
    storage::put_item(&mut *tx, acc, &[11u8; 16], 0, false, b"b").await.unwrap();
    storage::put_item(&mut *tx, acc, &[12u8; 16], 0, false, b"c").await.unwrap();

    // limit caps the page and results are ordered by seq ascending.
    let page = storage::pull(&mut *tx, acc, 0, 2).await.unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].seq, 1);
    assert_eq!(page[1].seq, 2);

    // since filters to strictly newer rows.
    let rest = storage::pull(&mut *tx, acc, 1, 500).await.unwrap();
    assert_eq!(rest.len(), 2);
    assert_eq!(rest[0].seq, 2);
    assert_eq!(rest[1].seq, 3);
}

#[tokio::test]
async fn storage_is_scoped_to_account() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let a = server::db::accounts::create(&mut *tx, "did:plc:storageiso0000001", None, false).await.unwrap();
    let b = server::db::accounts::create(&mut *tx, "did:plc:storageiso0000002", None, false).await.unwrap();

    storage::put_item(&mut *tx, a, &[20u8; 16], 0, false, b"a-only").await.unwrap();

    // Account b's store never sees account a's records.
    let b_items = storage::pull(&mut *tx, b, 0, 500).await.unwrap();
    assert!(b_items.is_empty());

    let (b_bytes, b_count) = storage::account_usage(&mut *tx, b).await.unwrap();
    assert_eq!(b_bytes, 0);
    assert_eq!(b_count, 0);
}

#[tokio::test]
async fn storage_account_usage_excludes_tombstones() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storageusage00001", None, false).await.unwrap();
    storage::put_item(&mut *tx, acc, &[30u8; 16], 0, false, b"12345").await.unwrap();
    storage::put_item(&mut *tx, acc, &[31u8; 16], 0, false, b"678").await.unwrap();

    let (bytes, count) = storage::account_usage(&mut *tx, acc).await.unwrap();
    assert_eq!(bytes, 8);
    assert_eq!(count, 2);

    // Tombstoning a record removes it from the live byte/count totals.
    storage::put_item(&mut *tx, acc, &[30u8; 16], 1, true, b"").await.unwrap();
    let (bytes, count) = storage::account_usage(&mut *tx, acc).await.unwrap();
    assert_eq!(bytes, 3);
    assert_eq!(count, 1);
}

// ── Storage snapshot tests (docs/05 §7) ──────────────────────────────────────

use server::db::storage::SnapshotOutcome;

#[tokio::test]
async fn storage_snapshot_absent_then_stored() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storagesnap00001", None, false).await.unwrap();

    // No snapshot yet.
    assert!(storage::get_snapshot(&mut *tx, acc).await.unwrap().is_none());

    // First push always wins (no row to compare against).
    match storage::put_snapshot(&mut *tx, acc, 5, b"snap-v5").await.unwrap() {
        SnapshotOutcome::Stored { snapshot_version } => assert_eq!(snapshot_version, 5),
        SnapshotOutcome::Stale { .. } => panic!("first snapshot should store"),
    }

    let snap = storage::get_snapshot(&mut *tx, acc).await.unwrap().unwrap();
    assert_eq!(snap.snapshot_version, 5);
    assert_eq!(snap.blob, b"snap-v5");
}

#[tokio::test]
async fn storage_snapshot_lww_newer_wins_stale_rejected() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let acc = server::db::accounts::create(&mut *tx, "did:plc:storagesnap00002", None, false).await.unwrap();

    storage::put_snapshot(&mut *tx, acc, 10, b"v10").await.unwrap();

    // A strictly newer version replaces the blob.
    match storage::put_snapshot(&mut *tx, acc, 11, b"v11").await.unwrap() {
        SnapshotOutcome::Stored { snapshot_version } => assert_eq!(snapshot_version, 11),
        SnapshotOutcome::Stale { .. } => panic!("newer snapshot should store"),
    }
    let snap = storage::get_snapshot(&mut *tx, acc).await.unwrap().unwrap();
    assert_eq!(snap.blob, b"v11");

    // An equal version is not strictly newer → rejected, blob untouched.
    match storage::put_snapshot(&mut *tx, acc, 11, b"v11-dup").await.unwrap() {
        SnapshotOutcome::Stale { current_version } => assert_eq!(current_version, 11),
        SnapshotOutcome::Stored { .. } => panic!("equal version should not store"),
    }
    // An older version is likewise rejected.
    match storage::put_snapshot(&mut *tx, acc, 9, b"v9").await.unwrap() {
        SnapshotOutcome::Stale { current_version } => assert_eq!(current_version, 11),
        SnapshotOutcome::Stored { .. } => panic!("older version should not store"),
    }

    // The stored snapshot is still v11's blob, never the rejected pushes.
    let snap = storage::get_snapshot(&mut *tx, acc).await.unwrap().unwrap();
    assert_eq!(snap.snapshot_version, 11);
    assert_eq!(snap.blob, b"v11");
}

#[tokio::test]
async fn storage_snapshot_is_scoped_to_account() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let a = server::db::accounts::create(&mut *tx, "did:plc:storagesnap00003", None, false).await.unwrap();
    let b = server::db::accounts::create(&mut *tx, "did:plc:storagesnap00004", None, false).await.unwrap();

    storage::put_snapshot(&mut *tx, a, 1, b"a-snap").await.unwrap();

    // Account b has no snapshot of its own.
    assert!(storage::get_snapshot(&mut *tx, b).await.unwrap().is_none());
}

// ── Projects / capabilities / tokens / events (docs/22, 24) ──────────────────

use server::db::{capabilities, projects, server_events, token_redemptions};

#[tokio::test]
async fn project_create_find_delete() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let id = projects::create(&mut *tx, "proj-cfd", "Project", Some("https://x.test")).await.unwrap();
    let found = projects::find_by_slug(&mut *tx, "proj-cfd").await.unwrap().unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.name, "Project");
    assert_eq!(found.url.as_deref(), Some("https://x.test"));
    assert!(found.signing_public_key.is_none());

    assert!(projects::delete_by_slug(&mut *tx, "proj-cfd").await.unwrap());
    assert!(projects::find_by_slug(&mut *tx, "proj-cfd").await.unwrap().is_none());
    assert!(!projects::delete_by_slug(&mut *tx, "proj-cfd").await.unwrap());
}

#[tokio::test]
async fn ensure_adminbot_project_idempotent() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let a = projects::ensure_adminbot_project(&mut *tx, "adminbot").await.unwrap();
    let b = projects::ensure_adminbot_project(&mut *tx, "adminbot").await.unwrap();
    assert_eq!(a, b, "ensure must be idempotent");
}

#[tokio::test]
async fn project_bots_one_to_many_and_resolution() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let p1 = projects::create(&mut *tx, "proj-otm-1", "P1", None).await.unwrap();
    let p2 = projects::create(&mut *tx, "proj-otm-2", "P2", None).await.unwrap();
    let bot_a = server::db::accounts::create(&mut *tx, "did:local:botm1", None, true).await.unwrap();
    let bot_b = server::db::accounts::create(&mut *tx, "did:local:botm2", None, true).await.unwrap();

    // One Project, many bots.
    projects::link_bot(&mut *tx, p1, bot_a).await.unwrap();
    projects::link_bot(&mut *tx, p1, bot_b).await.unwrap();
    let dids = projects::bot_dids(&mut *tx, p1).await.unwrap();
    assert_eq!(dids.len(), 2);

    // Resolution: account -> its project.
    let (pid, slug) = projects::project_for_account(&mut *tx, bot_a).await.unwrap().unwrap();
    assert_eq!(pid, p1);
    assert_eq!(slug, "proj-otm-1");

    // Re-linking a bot moves it to another Project (a bot is in <=1 Project).
    projects::link_bot(&mut *tx, p2, bot_a).await.unwrap();
    let (pid2, _) = projects::project_for_account(&mut *tx, bot_a).await.unwrap().unwrap();
    assert_eq!(pid2, p2);
    assert_eq!(projects::bot_dids(&mut *tx, p1).await.unwrap().len(), 1);

    // Unlink.
    assert!(projects::unlink_bot(&mut *tx, bot_a).await.unwrap());
    assert!(projects::project_for_account(&mut *tx, bot_a).await.unwrap().is_none());
}

#[tokio::test]
async fn capabilities_grant_check_revoke() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let p = projects::create(&mut *tx, "proj-cap", "P", None).await.unwrap();
    let bot = server::db::accounts::create(&mut *tx, "did:local:capbot1", None, true).await.unwrap();
    projects::link_bot(&mut *tx, p, bot).await.unwrap();

    assert!(!capabilities::account_has_capability(&mut *tx, bot, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());

    capabilities::grant(&mut *tx, p, capabilities::SUBSCRIBE_ACCOUNT_JOINED, "did:local:admin").await.unwrap();
    // Idempotent re-grant.
    capabilities::grant(&mut *tx, p, capabilities::SUBSCRIBE_ACCOUNT_JOINED, "did:local:admin").await.unwrap();
    assert!(capabilities::account_has_capability(&mut *tx, bot, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());
    assert_eq!(capabilities::list(&mut *tx, p).await.unwrap(), vec![capabilities::SUBSCRIBE_ACCOUNT_JOINED.to_string()]);

    assert!(capabilities::revoke(&mut *tx, p, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());
    assert!(!capabilities::account_has_capability(&mut *tx, bot, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());
    assert!(!capabilities::revoke(&mut *tx, p, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());
}

#[tokio::test]
async fn adminbot_superuser_short_circuit() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let pid = projects::ensure_adminbot_project(&mut *tx, "adminbot").await.unwrap();
    let bot = server::db::accounts::create(&mut *tx, "did:local:superu1", None, true).await.unwrap();
    projects::link_bot(&mut *tx, pid, bot).await.unwrap();

    // No capability rows granted, yet the adminbot Project's bot holds all.
    assert!(capabilities::account_has_capability(&mut *tx, bot, capabilities::SUBSCRIBE_ACCOUNT_JOINED).await.unwrap());
    assert!(capabilities::account_has_capability(&mut *tx, bot, capabilities::REGISTRATION_GATEKEEPER).await.unwrap());
}

// Note: `any_gatekeeper_exists` is a *global* existence check, so it can't be
// asserted under the transaction-rollback pattern on the shared dev DB
// (http_tests commits gatekeeper grants that this would observe). Its behavior
// — and the shared-secret auto-disable/re-enable it drives — is covered
// end-to-end by `closed_registration_admission_matrix` in http_tests.rs.

#[tokio::test]
async fn gatekeeper_signing_key_set_and_clear() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let p = projects::create(&mut *tx, "proj-gk", "GK", None).await.unwrap();
    projects::set_signing_key(&mut *tx, p, Some(&[7u8; 32])).await.unwrap();
    let found = projects::find_by_slug(&mut *tx, "proj-gk").await.unwrap().unwrap();
    assert_eq!(found.signing_public_key.as_deref(), Some(&[7u8; 32][..]));

    projects::set_signing_key(&mut *tx, p, None).await.unwrap();
    assert!(projects::find_by_slug(&mut *tx, "proj-gk").await.unwrap().unwrap().signing_public_key.is_none());
}

#[tokio::test]
async fn token_redemption_is_single_use() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    assert!(!token_redemptions::is_redeemed(&mut *tx, "jti-su-1").await.unwrap());
    assert!(token_redemptions::try_redeem(&mut *tx, "jti-su-1", "gk", "invite", "did:plc:x").await.unwrap());
    assert!(token_redemptions::is_redeemed(&mut *tx, "jti-su-1").await.unwrap());
    // Replay conflicts.
    assert!(!token_redemptions::try_redeem(&mut *tx, "jti-su-1", "gk", "invite", "did:plc:y").await.unwrap());
}

#[tokio::test]
async fn server_events_append_and_fetch() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let id1 = server_events::append_account_joined(&mut *tx, "did:plc:ev1", Some("tok-1"), 1000).await.unwrap();
    let id2 = server_events::append_account_joined(&mut *tx, "did:plc:ev2", None, 2000).await.unwrap();
    assert!(id2 > id1);

    // Filter to our own events rather than asserting raw counts — the shared
    // dev DB carries many committed events from other tests. Window from just
    // below id1 so the LIMIT can't truncate ours (they're the lowest ids in it).
    let kind = server_events::KIND_ACCOUNT_JOINED;
    let window = server_events::fetch_since(&mut *tx, id1 - 1, kind, 500).await.unwrap();
    let e1 = window.iter().find(|e| e.id == id1).expect("ev1 present");
    let e2 = window.iter().find(|e| e.id == id2).expect("ev2 present");
    assert_eq!(e1.did, "did:plc:ev1");
    assert_eq!(e1.invite_token.as_deref(), Some("tok-1"));
    assert_eq!(e2.did, "did:plc:ev2");
    assert!(e2.invite_token.is_none());

    // fetch_since is strictly-greater-than: id1 is excluded, id2 included.
    let after = server_events::fetch_since(&mut *tx, id1, kind, 500).await.unwrap();
    assert!(after.iter().any(|e| e.id == id2), "id2 after id1");
    assert!(!after.iter().any(|e| e.id == id1), "id1 excluded by since=id1");
}

#[tokio::test]
async fn delete_account_unlinks_from_project() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let p = projects::create(&mut *tx, "proj-del", "P", None).await.unwrap();
    let bot = server::db::accounts::create(&mut *tx, "did:local:delbot1", None, true).await.unwrap();
    projects::link_bot(&mut *tx, p, bot).await.unwrap();

    server::db::accounts::delete_account(&mut *tx, bot).await.unwrap();

    // Bot link is gone; the Project row survives.
    assert!(projects::project_for_account(&mut *tx, bot).await.unwrap().is_none());
    assert!(projects::find_by_slug(&mut *tx, "proj-del").await.unwrap().is_some());
}

// ── Abuse reports (docs/12 §3) ─────────────────────────────────────────────────

#[tokio::test]
async fn abuse_report_insert_and_count() {
    use server::db::{abuse, accounts};
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let reporter = accounts::create(&mut *tx, "did:plc:abusereporter01", None, false)
        .await
        .unwrap();

    assert_eq!(abuse::count_by_reporter(&mut *tx, reporter).await.unwrap(), 0);

    let id = abuse::insert(&mut *tx, "did:plc:spammer1", "spam", reporter).await.unwrap();
    assert!(id > 0);
    abuse::insert(&mut *tx, "did:plc:spammer1", "harassment", reporter).await.unwrap();

    // Per-reporter count drives the rate limit / operator audit.
    assert_eq!(abuse::count_by_reporter(&mut *tx, reporter).await.unwrap(), 2);

    // Operator-review listing returns both, newest first, no message content.
    let reports = abuse::list_for_did(&mut *tx, "did:plc:spammer1").await.unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].reason, "harassment");
    assert_eq!(reports[1].reason, "spam");
    assert!(reports.iter().all(|r| r.reporter_account == reporter));
}

#[tokio::test]
async fn delete_account_with_filed_abuse_report_succeeds() {
    // Regression: `abuse_reports.reporter_account` is NOT NULL with no cascade,
    // so deleting an account that has filed a report must drop those reports in
    // the cascade or the root `DELETE FROM accounts` FK-violates.
    use server::db::{abuse, accounts};
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let reporter = accounts::create(&mut *tx, "did:plc:delreporter01", None, false)
        .await
        .unwrap();
    let other = accounts::create(&mut *tx, "did:plc:delreporter02", None, false)
        .await
        .unwrap();

    // `reporter` files reports; a different account also reports `reporter`'s DID.
    abuse::insert(&mut *tx, "did:plc:somespammer", "spam", reporter).await.unwrap();
    abuse::insert(&mut *tx, "did:plc:somespammer", "harassment", reporter).await.unwrap();
    abuse::insert(&mut *tx, "did:plc:delreporter01", "spam", other).await.unwrap();

    // Deleting the reporter succeeds and removes the reports it filed.
    accounts::delete_account(&mut *tx, reporter).await.unwrap();
    assert_eq!(abuse::count_by_reporter(&mut *tx, reporter).await.unwrap(), 0);
    assert!(accounts::find_by_did(&mut *tx, "did:plc:delreporter01").await.unwrap().is_none());

    // Reports filed *about* the deleted DID (plaintext `reported_did`, no FK)
    // are retained for operator review.
    let about = abuse::list_for_did(&mut *tx, "did:plc:delreporter01").await.unwrap();
    assert_eq!(about.len(), 1, "reports about the deleted account survive");
    assert_eq!(about[0].reporter_account, other);
}

// ── Provisioning (device-linking mailbox) tests ──────────────────────────────

#[tokio::test]
async fn provisioning_session_create_and_slot_round_trip() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    server::db::provisioning::create_session(&mut *tx, "sess-rt-001", 300).await.unwrap();

    // A slot starts empty.
    assert!(server::db::provisioning::get_slot(&mut *tx, "sess-rt-001", "handshake")
        .await
        .unwrap()
        .is_none());

    // Put then get.
    let stored = server::db::provisioning::put_slot(&mut *tx, "sess-rt-001", "handshake", b"abc")
        .await
        .unwrap();
    assert!(stored);
    let got = server::db::provisioning::get_slot(&mut *tx, "sess-rt-001", "handshake")
        .await
        .unwrap();
    assert_eq!(got.as_deref(), Some(&b"abc"[..]));

    // Overwrite (upsert) the same slot.
    server::db::provisioning::put_slot(&mut *tx, "sess-rt-001", "handshake", b"xyz")
        .await
        .unwrap();
    let got = server::db::provisioning::get_slot(&mut *tx, "sess-rt-001", "handshake")
        .await
        .unwrap();
    assert_eq!(got.as_deref(), Some(&b"xyz"[..]));

    // A different slot is independent.
    server::db::provisioning::put_slot(&mut *tx, "sess-rt-001", "bundle", b"sealed")
        .await
        .unwrap();
    assert_eq!(
        server::db::provisioning::get_slot(&mut *tx, "sess-rt-001", "bundle").await.unwrap().as_deref(),
        Some(&b"sealed"[..])
    );
}

#[tokio::test]
async fn provisioning_put_to_missing_session_is_rejected() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let stored = server::db::provisioning::put_slot(&mut *tx, "no-such-session", "handshake", b"x")
        .await
        .unwrap();
    assert!(!stored, "writing to a nonexistent session must not store anything");
    assert!(server::db::provisioning::get_slot(&mut *tx, "no-such-session", "handshake")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn provisioning_expired_session_hides_slots() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    // Create a session that is already expired (negative lifetime).
    server::db::provisioning::create_session(&mut *tx, "sess-expired-01", -10).await.unwrap();

    // Both writing and reading gate on a live session, so neither succeeds.
    let stored = server::db::provisioning::put_slot(&mut *tx, "sess-expired-01", "handshake", b"x")
        .await
        .unwrap();
    assert!(!stored, "cannot write to an expired session");
    assert!(server::db::provisioning::get_slot(&mut *tx, "sess-expired-01", "handshake")
        .await
        .unwrap()
        .is_none());

    // delete_expired sweeps it.
    let swept = server::db::provisioning::delete_expired(&mut *tx).await.unwrap();
    assert!(swept >= 1);
}

// ── Per-device group push pseudonyms (docs/04 multi-device groups) ──────────────

/// Insert a minimal group so the FK on `group_member_pseudonyms.group_id` holds.
async fn setup_group(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, gid: &[u8], founder_emi: &[u8], founder_ps: &[u8]) {
    let policy = server::db::groups::Policy {
        invite_members_role: 1,
        remove_members_role: 1,
        modify_title_role: 1,
        modify_description_role: 1,
        modify_expiry_role: 1,
        join_policy: 0,
        invite_link_password: None,
        announcement_only: false,
    };
    server::db::groups::create(
        &mut **tx,
        &server::db::groups::NewGroup {
            group_id: gid,
            server_public_params_version: server::db::zkgroup_params::CURRENT_VERSION,
            group_public_params: &[1u8; 32],
            encrypted_state: &[2u8; 16],
            policy: &policy,
            founder_encrypted_member_id: founder_emi,
            founder_group_push_pseudonym: founder_ps,
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn member_pseudonyms_fan_out_to_all_devices() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let gid = b"group-fanout-0001".to_vec();
    let founder_emi = b"founder-emi".to_vec();
    let founder_ps = b"founder-pseudonym-aaaaaa".to_vec();
    setup_group(&mut tx, &gid, &founder_emi, &founder_ps).await;

    // Founder's create registered exactly their first device's pseudonym.
    let founder = server::db::groups::member_pseudonyms(&mut *tx, &gid, &founder_emi).await.unwrap();
    assert_eq!(founder, vec![founder_ps.clone()]);

    // A second member joins (one device), then links a second device.
    let emi = b"member-emi".to_vec();
    let ps1 = b"member-device-1-pseudonym".to_vec();
    let ps2 = b"member-device-2-pseudonym".to_vec();
    server::db::groups::insert_member(&mut *tx, &gid, &emi, 0).await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, &ps1).await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, &ps2).await.unwrap();

    // Fan-out resolves the member's EMI to BOTH device pseudonyms — the crux of
    // concurrent multi-device receive.
    let mut got = server::db::groups::member_pseudonyms(&mut *tx, &gid, &emi).await.unwrap();
    got.sort();
    let mut want = vec![ps1.clone(), ps2.clone()];
    want.sort();
    assert_eq!(got, want);

    // Re-registering an existing pseudonym is idempotent (ON CONFLICT DO NOTHING).
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, &ps1).await.unwrap();
    assert_eq!(server::db::groups::member_pseudonyms(&mut *tx, &gid, &emi).await.unwrap().len(), 2);

    // Ownership check gates offline pickup: own pseudonym yes, a stranger's no.
    assert!(server::db::groups::pseudonym_belongs_to_member(&mut *tx, &gid, &emi, &ps1).await.unwrap());
    assert!(!server::db::groups::pseudonym_belongs_to_member(&mut *tx, &gid, &emi, &founder_ps).await.unwrap());
}

#[tokio::test]
async fn rotate_pseudonym_replaces_only_that_device() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let gid = b"group-rotate-0001".to_vec();
    let founder_emi = b"founder-emi".to_vec();
    setup_group(&mut tx, &gid, &founder_emi, b"founder-ps-rotate-aaaaaa").await;

    let emi = b"member-emi".to_vec();
    let ps1 = b"device-1-old-pseudonym--".to_vec();
    let ps2 = b"device-2-pseudonym------".to_vec();
    server::db::groups::insert_member(&mut *tx, &gid, &emi, 0).await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, &ps1).await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, &ps2).await.unwrap();

    // Device 1 rotates old → new; device 2's binding is untouched.
    let ps1_new = b"device-1-new-pseudonym--".to_vec();
    let rotated = server::db::groups::rotate_member_pseudonym(&mut *tx, &gid, &emi, &ps1, &ps1_new)
        .await
        .unwrap();
    assert!(rotated);
    let mut got = server::db::groups::member_pseudonyms(&mut *tx, &gid, &emi).await.unwrap();
    got.sort();
    let mut want = vec![ps1_new.clone(), ps2.clone()];
    want.sort();
    assert_eq!(got, want);

    // Rotating a pseudonym we don't hold is a no-op miss (returns false).
    assert!(!server::db::groups::rotate_member_pseudonym(&mut *tx, &gid, &emi, &ps1, &ps1_new).await.unwrap());
}

#[tokio::test]
async fn delete_member_cascades_all_device_pseudonyms() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let gid = b"group-cascade-0001".to_vec();
    let founder_emi = b"founder-emi".to_vec();
    setup_group(&mut tx, &gid, &founder_emi, b"founder-ps-cascade-aaaaa").await;

    let emi = b"member-emi".to_vec();
    server::db::groups::insert_member(&mut *tx, &gid, &emi, 0).await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, b"dev-1-ps-cascade--------").await.unwrap();
    server::db::groups::insert_member_pseudonym(&mut *tx, &gid, &emi, b"dev-2-ps-cascade--------").await.unwrap();
    assert_eq!(server::db::groups::member_pseudonyms(&mut *tx, &gid, &emi).await.unwrap().len(), 2);

    // Removing the member drops the credential row AND every device pseudonym.
    server::db::groups::delete_member(&mut *tx, &gid, &emi).await.unwrap();
    assert!(server::db::groups::member_pseudonyms(&mut *tx, &gid, &emi).await.unwrap().is_empty());
}
