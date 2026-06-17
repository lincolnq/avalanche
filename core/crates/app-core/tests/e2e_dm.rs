//! End-to-end integration test: two clients exchange encrypted DMs through
//! a real homeserver.
//!
//! Requires:
//! - A running homeserver at SERVER_URL (default: http://localhost:3000)
//! - The test database (Postgres) backing it
//!
//! Each test creates fresh accounts so they don't interfere with each other.

mod common;

use app_core::{AppCore, MessageTarget};
use std::path::Path;

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
async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

/// Filter to messages from `sender_did`. The dev homeserver runs adminbot,
/// which welcomes every new account; without filtering, every receive_messages
/// call surfaces those welcome DMs alongside the test traffic.
fn only_from(
    msgs: &[app_core::DecryptedMessage],
    sender_did: &str,
) -> Vec<app_core::DecryptedMessage> {
    msgs.iter().filter(|m| m.sender_did == sender_did).cloned().collect()
}

#[tokio::test]
async fn alice_sends_dm_to_bob() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();

    let bob_did = bob.did_async().await;

    let plaintext = b"Hello Bob, this is a secret message!";
    alice.send_dm_async(&bob_did, plaintext, now_ms()).await.unwrap();

    let messages = bob.receive_messages_async().await.unwrap();
    let messages = only_from(&messages, &alice.did_async().await);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].plaintext, plaintext);
    assert_eq!(messages[0].sender_did, alice.did_async().await);
    assert_eq!(messages[0].sender_device_id, alice.device_id_async().await);

    let messages2 = only_from(
        &bob.receive_messages_async().await.unwrap(),
        &alice.did_async().await,
    );
    assert!(messages2.is_empty());
}

#[tokio::test]
async fn bidirectional_conversation() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();

    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // Alice → Bob (PreKey message, establishes session).
    alice.send_dm_async(&bob_did, b"Hey Bob", now_ms()).await.unwrap();
    let msgs = only_from(&bob.receive_messages_async().await.unwrap(), &alice_did);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Hey Bob");

    // Bob → Alice (Bob's first message back, also a PreKey message).
    bob.send_dm_async(&alice_did, b"Hey Alice", now_ms()).await.unwrap();
    let msgs = only_from(&alice.receive_messages_async().await.unwrap(), &bob_did);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Hey Alice");

    // Alice → Bob (Whisper message, session established).
    alice.send_dm_async(&bob_did, b"How are you?", now_ms()).await.unwrap();
    let msgs = only_from(&bob.receive_messages_async().await.unwrap(), &alice_did);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"How are you?");

    // Bob → Alice (Whisper message).
    bob.send_dm_async(&alice_did, b"Doing great!", now_ms()).await.unwrap();
    let msgs = only_from(&alice.receive_messages_async().await.unwrap(), &bob_did);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].plaintext, b"Doing great!");
}

#[tokio::test]
async fn multiple_messages_in_one_fetch() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();

    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // Alice sends 3 messages before Bob fetches.
    alice.send_dm_async(&bob_did, b"msg1", now_ms()).await.unwrap();
    alice.send_dm_async(&bob_did, b"msg2", now_ms()).await.unwrap();
    alice.send_dm_async(&bob_did, b"msg3", now_ms()).await.unwrap();

    let msgs = only_from(&bob.receive_messages_async().await.unwrap(), &alice_did);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].plaintext, b"msg1");
    assert_eq!(msgs[1].plaintext, b"msg2");
    assert_eq!(msgs[2].plaintext, b"msg3");
}

#[tokio::test]
async fn login_re_authenticates() {
    // Verifies the challenge-response auth flow works end-to-end.
    // Regression test for the broken `authenticate()` that sent the old
    // pre-challenge-response format and always got a 422.
    let url = server_url();

    // Use a temp file-backed store so state survives across two Store::open calls.
    let db_path = std::env::temp_dir().join(format!(
        "actnet-test-login-{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));

    let (_id1, store1) = store::open_split(
        Path::new(&db_path),
        &store::DatabaseKey::from_passphrase("test-key".to_string()),
    )
    .await
    .unwrap();
    AppCore::create_account_with_store(&url, store1, None, true, common::invite_token())
        .await
        .unwrap();

    // Re-open the same DB and login — exercises the challenge-response flow.
    let (_id2, store2) = store::open_split(
        Path::new(&db_path),
        &store::DatabaseKey::from_passphrase("test-key".to_string()),
    )
    .await
    .unwrap();
    let core = AppCore::login_with_store(store2).await.unwrap();

    // A successful fetch proves the session token is valid.
    core.receive_messages_async().await.unwrap();

    let _ = std::fs::remove_file(&db_path);
}

/// Full wire round-trip for a reaction (docs/33): build → encrypt → server →
/// decrypt → dispatch → store, on both the reactor's and the recipient's side.
/// Edits and deletes ride the identical ContentMessage transport + dispatch;
/// their store effects are covered by the store unit tests.
#[tokio::test]
async fn alice_reacts_to_a_message_bob_sees_it() {
    let url = server_url();
    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // Alice reacts 👍 to one of Bob's messages (identified by author+sent_at).
    let target_sent_at = 1_700_000_000_000i64;
    alice.send_reaction_async(MessageTarget::Dm { recipient_did: bob_did.clone() }, &bob_did, target_sent_at, "👍", false, now_ms()).await.unwrap();

    // Reactor's own store reflects it immediately.
    let alice_conv = format!("dm-{alice_did}-{bob_did}");
    let ar = alice.load_reactions_async(&alice_conv).await.unwrap();
    assert_eq!(ar.len(), 1);
    assert_eq!(ar[0].emoji, "👍");
    assert_eq!(ar[0].reactor_did, alice_did);

    // Bob receives and applies it (keyed on the target's wire identity).
    bob.receive_messages_async().await.unwrap();
    let bob_conv = format!("dm-{bob_did}-{alice_did}");
    let br = bob.load_reactions_async(&bob_conv).await.unwrap();
    assert_eq!(br.len(), 1);
    assert_eq!(br[0].emoji, "👍");
    assert_eq!(br[0].reactor_did, alice_did);
    assert_eq!(br[0].target_author, bob_did);

    // Re-reacting replaces (one per person per message).
    alice.send_reaction_async(MessageTarget::Dm { recipient_did: bob_did.clone() }, &bob_did, target_sent_at, "❤️", false, now_ms()).await.unwrap();
    bob.receive_messages_async().await.unwrap();
    let br = bob.load_reactions_async(&bob_conv).await.unwrap();
    assert_eq!(br.len(), 1);
    assert_eq!(br[0].emoji, "❤️");

    // Removing clears it.
    alice.send_reaction_async(MessageTarget::Dm { recipient_did: bob_did.clone() }, &bob_did, target_sent_at, "❤️", true, now_ms()).await.unwrap();
    bob.receive_messages_async().await.unwrap();
    assert!(bob.load_reactions_async(&bob_conv).await.unwrap().is_empty());
}

/// Message-request gate + block + report, end to end (docs/12 §1–3).
#[tokio::test]
async fn message_request_block_and_report() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token()).await.unwrap();
    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // 1. Alice, a stranger to Bob, makes first contact. A request is still
    //    delivered (the gate surfaces it; it doesn't drop), and Bob now has a
    //    pending, un-curated, un-blocked row for Alice.
    alice.send_dm_async(&bob_did, b"hi, buy my coins", now_ms()).await.unwrap();
    let msgs = only_from(&bob.receive_messages_async().await.unwrap(), &alice_did);
    assert_eq!(msgs.len(), 1, "a first-contact request is still delivered");
    let (curated, blocked, pending) = bob.contact_state_async(&alice_did).await.unwrap();
    assert!(!curated && !blocked && pending, "stranger's first contact is a pending request");

    // Bob evaluates but does not accept: no read receipt flows to Alice. The
    // suppressed receipt means Alice's next fetch surfaces nothing from Bob
    // (the auto delivery receipt is consumed internally, never an event).
    bob.send_read_receipt_async(&alice_did, vec![now_ms()]).await.unwrap();
    assert!(
        only_from(&alice.receive_messages_async().await.unwrap(), &bob_did).is_empty(),
        "no read receipt is sent for an un-accepted request"
    );

    // 2. Bob reports + blocks Alice. The report lands on Bob's homeserver; the
    //    block is local and clears the pending request.
    bob.report_and_block_async(&alice_did, "spam").await.unwrap();
    let (_c, blocked, pending) = bob.contact_state_async(&alice_did).await.unwrap();
    assert!(blocked && !pending, "report-and-block sets is_blocked and clears the request");

    // 3. Alice messages again; Bob drops it after decryption — no event.
    alice.send_dm_async(&bob_did, b"please respond", now_ms()).await.unwrap();
    let msgs = only_from(&bob.receive_messages_async().await.unwrap(), &alice_did);
    assert!(msgs.is_empty(), "messages from a blocked sender are dropped");

    // 4. Bob's own outgoing message to the blocked Alice is refused locally.
    let err = bob.send_dm_async(&alice_did, b"nope", now_ms()).await;
    assert!(
        matches!(err, Err(app_core::error::AppError::Blocked(_))),
        "outgoing to a blocked contact is refused, got {err:?}"
    );

    // 5. Unblock restores sending.
    bob.block_contact_async(&alice_did, false).await.unwrap();
    bob.send_dm_async(&alice_did, b"ok let's talk", now_ms()).await.unwrap();
}
