//! End-to-end integration test: one-time prekey exhaustion + replenishment.
//!
//! Requires:
//! - A running homeserver at SERVER_URL (default: http://localhost:3000)
//! - The test database (Postgres) backing it
//!
//! Run via `make test-e2e`. Each test creates fresh accounts.

use app_core::AppCore;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn test_store() -> store::Store {
    let store = store::Store::open_in_memory().await.unwrap();
    store.migrate().await.unwrap();
    store
}

fn only_from(
    msgs: &[app_core::DecryptedMessage],
    sender_did: &str,
) -> Vec<app_core::DecryptedMessage> {
    msgs.iter().filter(|m| m.sender_did == sender_did).cloned().collect()
}

/// Drain a recipient's one-time prekey pools, then confirm a brand-new sender's
/// first-contact message still decrypts, and that replenishment refills.
///
/// This is the regression for the "no opening message" bug: once the one-time
/// pools are empty the server serves the signed EC + **last-resort Kyber**
/// fallback. That used to fail to decrypt because the last-resort Kyber shared
/// id `1` with a one-time Kyber in the recipient's local store (one clobbered
/// the other). With the last-resort moved to a distinct id, the fallback
/// decrypts; with replenishment, the pools never reach empty in the first place.
#[tokio::test]
async fn first_contact_decrypts_after_exhaustion_then_replenishes() {
    let url = server_url();

    // Recipient. `create_account_with_store` does not start the reconnect task,
    // so sends/receives use the HTTP path — deterministic for the test.
    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();
    let alice_did = alice.did_async().await;

    // Drain: 25 distinct senders each open a first-contact session, consuming
    // one one-time EC + one one-time Kyber prekey apiece. Registration mints 20
    // of each, so by the end both one-time pools are empty and the server is
    // handing out the signed-EC + last-resort-Kyber fallback.
    for i in 0..25 {
        let sender = AppCore::create_account_with_store(&url, test_store().await, None, true)
            .await
            .unwrap();
        sender
            .send_dm_async(&alice_did, format!("drain {i}").as_bytes(), now_ms())
            .await
            .unwrap();
    }

    // The decisive sender, created after exhaustion: its bundle carries the
    // last-resort Kyber. Its first-contact message must decrypt on Alice.
    let late = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();
    let late_did = late.did_async().await;
    late.send_dm_async(&alice_did, b"after exhaustion", now_ms())
        .await
        .unwrap();

    let msgs = alice.receive_messages_async().await.unwrap();
    let from_late = only_from(&msgs, &late_did);
    assert_eq!(
        from_late.len(),
        1,
        "first-contact message after pool exhaustion must decrypt (last-resort Kyber path)"
    );
    assert_eq!(from_late[0].plaintext, b"after exhaustion");

    // Pools are drained; replenishment should refill both above the threshold.
    let (ec_before, kyber_before) = alice.prekey_status_async().await.unwrap();
    assert!(ec_before < 10, "EC pool should be drained, got {ec_before}");
    assert!(kyber_before < 10, "Kyber pool should be drained, got {kyber_before}");

    alice.replenish_prekeys_async().await;

    let (ec_after, kyber_after) = alice.prekey_status_async().await.unwrap();
    assert!(ec_after >= 10, "EC pool should be refilled, got {ec_after}");
    assert!(kyber_after >= 10, "Kyber pool should be refilled, got {kyber_after}");
}
