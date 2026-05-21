//! Database layer tests using the transaction-rollback pattern.
//!
//! Each test begins a transaction, runs its assertions, then the transaction
//! is dropped (rolled back) on exit. This means:
//! - Tests are fast (no per-test DB setup/teardown).
//! - Tests are isolated (writes never commit, so tests don't interfere).
//! - Tests need a running Postgres with the schema already applied.
//!
//! Set `TEST_DATABASE_URL` to point at a test Postgres instance.
//! The schema from `infra/migrations/001_initial.sql` must be applied first.

use sqlx::PgPool;

/// Connect to the test database. Panics if TEST_DATABASE_URL is not set.
async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run server tests");
    PgPool::connect(&url).await.expect("failed to connect to test database")
}

/// Begin a transaction that will be rolled back when dropped.
/// Returns a connection usable as `&mut PgConnection` via `&mut *tx`.
async fn begin_tx(pool: &PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
    pool.begin().await.expect("failed to begin transaction")
}

// â”€â”€ Account tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
}

#[tokio::test]
async fn find_account_nonexistent_returns_none() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    let found = server::db::accounts::find_by_did(&mut *tx, "did:plc:doesnotexist0000").await.unwrap();
    assert!(found.is_none());
}

// â”€â”€ Device tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Session token tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ DID document tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Prekey tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Message queue tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Prekey vacuum tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn prekey_vacuum_sends_notification_when_below_threshold() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumlow000000001").await;

    // Upload 2 one-time prekeys and 1 Kyber â€” both below threshold of 10.
    let otpks = vec![(1, vec![20u8; 32]), (2, vec![21u8; 32])];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 1, &[30u8; 32], &[31u8; 64]).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("expected a prekey_low notification");
    let parsed: serde_json::Value = serde_json::from_str(&msg.0).unwrap();
    assert_eq!(parsed["type"], "prekey_low");
    assert_eq!(parsed["one_time_remaining"], 2);
    assert_eq!(parsed["kyber_remaining"], 1);
}

#[tokio::test]
async fn prekey_vacuum_no_notification_when_both_above_threshold() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumhigh00000001").await;

    // Upload 20 one-time prekeys and 15 Kyber â€” both above threshold of 10.
    let otpks: Vec<(i32, Vec<u8>)> = (1..=20).map(|i| (i, vec![i as u8; 32])).collect();
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    for i in 1..=15i32 {
        server::db::prekeys::upsert_kyber(&mut *tx, device_pk, i, &[i as u8; 32], &[31u8; 64]).await.unwrap();
    }

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    assert!(rx_ws.try_recv().is_err(), "both counts above threshold, expected no notification");
}

#[tokio::test]
async fn prekey_vacuum_notifies_when_only_ec_low() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumeclow0000001").await;

    // 2 EC prekeys (below 10), 15 Kyber (above 10).
    let otpks = vec![(1, vec![20u8; 32]), (2, vec![21u8; 32])];
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    for i in 1..=15i32 {
        server::db::prekeys::upsert_kyber(&mut *tx, device_pk, i, &[i as u8; 32], &[31u8; 64]).await.unwrap();
    }

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("EC count below threshold should trigger notification");
    let parsed: serde_json::Value = serde_json::from_str(&msg.0).unwrap();
    assert_eq!(parsed["type"], "prekey_low");
    assert_eq!(parsed["one_time_remaining"], 2);
}

#[tokio::test]
async fn prekey_vacuum_notifies_when_only_kyber_low() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;
    let device_pk = setup_device(&mut tx, "did:plc:vacuumkyblow000001").await;

    // 20 EC prekeys (above 10), 1 Kyber (below 10).
    let otpks: Vec<(i32, Vec<u8>)> = (1..=20).map(|i| (i, vec![i as u8; 32])).collect();
    server::db::prekeys::insert_one_time_batch(&mut *tx, device_pk, &otpks).await.unwrap();
    server::db::prekeys::upsert_kyber(&mut *tx, device_pk, 1, &[30u8; 32], &[31u8; 64]).await.unwrap();

    let (tx_ws, mut rx_ws) = tokio::sync::mpsc::unbounded_channel();
    server::tasks::notify_if_prekeys_low(&mut *tx, device_pk, 10, &tx_ws).await.unwrap();

    let msg = rx_ws.try_recv().expect("Kyber count below threshold should trigger notification");
    let parsed: serde_json::Value = serde_json::from_str(&msg.0).unwrap();
    assert_eq!(parsed["type"], "prekey_low");
    assert_eq!(parsed["kyber_remaining"], 1);
}

// â”€â”€ Project token tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Auth challenge tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Auth challenge + signature integration test â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

    // Consume the challenge and verify the signature â€” mirrors the handler logic.
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

    // Sign with a different keypair â€” wrong key.
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
