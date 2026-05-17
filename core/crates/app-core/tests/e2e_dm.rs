//! End-to-end integration test: two clients exchange encrypted DMs through
//! a real homeserver.
//!
//! Requires:
//! - A running homeserver at SERVER_URL (default: http://localhost:3000)
//! - The test database (Postgres) backing it
//!
//! Each test creates fresh accounts so they don't interfere with each other.

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

/// Create an in-memory store for testing (no disk I/O).
async fn test_store() -> store::Store {
    let store = store::Store::open_in_memory().await.unwrap();
    store.migrate().await.unwrap();
    store
}

#[tokio::test]
async fn alice_sends_dm_to_bob() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();

    let bob_did = bob.did_async().await;
    let bob_device = bob.device_id_async().await;

    let plaintext = b"Hello Bob, this is a secret message!";
    alice.send_dm_async(&bob_did, bob_device, plaintext, now_ms()).await.unwrap();

    let messages = bob.receive_messages_async().await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].plaintext, plaintext);
    assert_eq!(messages[0].sender_did, alice.did_async().await);
    assert_eq!(messages[0].sender_device_id, alice.device_id_async().await);

    let messages2 = bob.receive_messages_async().await.unwrap();
    assert!(messages2.is_empty());
}

#[tokio::test]
async fn bidirectional_conversation() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();

    let alice_did = alice.did_async().await;
    let alice_device = alice.device_id_async().await;
    let bob_did = bob.did_async().await;
    let bob_device = bob.device_id_async().await;

    // Alice → Bob (PreKey message, establishes session).
    alice.send_dm_async(&bob_did, bob_device, b"Hey Bob", now_ms()).await.unwrap();
    let msgs = bob.receive_messages_async().await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Hey Bob");

    // Bob → Alice (Bob's first message back, also a PreKey message).
    bob.send_dm_async(&alice_did, alice_device, b"Hey Alice", now_ms()).await.unwrap();
    let msgs = alice.receive_messages_async().await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Hey Alice");

    // Alice → Bob (Whisper message, session established).
    alice.send_dm_async(&bob_did, bob_device, b"How are you?", now_ms()).await.unwrap();
    let msgs = bob.receive_messages_async().await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"How are you?");

    // Bob → Alice (Whisper message).
    bob.send_dm_async(&alice_did, alice_device, b"Doing great!", now_ms()).await.unwrap();
    let msgs = alice.receive_messages_async().await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Doing great!");
}

#[tokio::test]
async fn multiple_messages_in_one_fetch() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await).await.unwrap();

    let bob_did = bob.did_async().await;
    let bob_device = bob.device_id_async().await;

    // Alice sends 3 messages before Bob fetches.
    alice.send_dm_async(&bob_did, bob_device, b"msg1", now_ms()).await.unwrap();
    alice.send_dm_async(&bob_did, bob_device, b"msg2", now_ms()).await.unwrap();
    alice.send_dm_async(&bob_did, bob_device, b"msg3", now_ms()).await.unwrap();

    let msgs = bob.receive_messages_async().await.unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].plaintext, b"msg1");
    assert_eq!(msgs[1].plaintext, b"msg2");
    assert_eq!(msgs[2].plaintext, b"msg3");
}
