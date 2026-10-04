//! End-to-end tests for group invites from people the invitee hasn't accepted
//! (docs/09 S-04, docs/12 §1). A stranger's invite is held as a request —
//! nothing joined, nothing published to the group — until the invitee joins
//! or declines; a blocked inviter's invite is discarded; an accepted contact's
//! invite still joins automatically (covered by the other group e2e files,
//! which `common::introduce` their accounts first).
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

mod common;

use app_core::{AppCore, IncomingEvent};
use std::sync::Arc;
use std::time::Duration;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn account() -> AppCore {
    AppCore::create_account_with_store(
        &server_url(),
        store::DeviceStore::open_in_memory().await.unwrap(),
        None,
        true,
        common::invite_token(),
    )
    .await
    .unwrap()
}

/// A stranger's invite (polling receive path) leaves the invitee a pending
/// invitee only, shows up as a request row, and joins only on accept.
#[tokio::test]
async fn stranger_invite_is_held_until_accepted() {
    let alice = account().await;
    let bob = account().await;
    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    let created = alice.create_group_async("Rally team", "", 0).await.unwrap();
    alice.invite_member_async(&created.group_id, &bob_did, 0).await.unwrap();
    bob.receive_messages_async().await.unwrap();

    // Not joined: Bob has no stored group, and the server still lists him only
    // as a pending invitee.
    assert!(
        bob.fetch_group_state_async(&created.group_id).await.is_err(),
        "a stranger's invite must not be stored as a joined group"
    );
    let state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(state.members.len(), 1, "bob must not have joined");
    assert_eq!(state.pending_invites.len(), 1);

    // It shows in Bob's chat list as a request naming the inviter.
    let conversation_id = format!("group-{}", created.group_id);
    let row = bob
        .load_conversations_async()
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.conversation_id == conversation_id)
        .expect("the invite appears in the chat list");
    assert!(row.is_request, "a stranger's group invite is a request");
    assert_eq!(row.inviter_did.as_deref(), Some(alice_did.as_str()));
    assert!(row.invited_at_ms.is_some(), "a request row carries its invite time");

    // Join: Bob becomes a member, and the request row becomes a normal group.
    bob.accept_invite_async(&created.group_id).await.unwrap();
    let state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(state.members.len(), 2, "bob joins on accept");
    assert!(state.pending_invites.is_empty());
    let row = bob
        .load_conversations_async()
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.conversation_id == conversation_id)
        .expect("the joined group stays in the chat list");
    assert!(!row.is_request);
    assert!(row.inviter_did.is_none());
}

/// Declining a stranger's invite removes it locally and tells the server, so
/// the inviter stops seeing the invitee as pending.
#[tokio::test]
async fn stranger_invite_can_be_declined() {
    let alice = account().await;
    let bob = account().await;
    let bob_did = bob.did_async().await;

    let created = alice.create_group_async("Not for me", "", 0).await.unwrap();
    alice.invite_member_async(&created.group_id, &bob_did, 0).await.unwrap();
    bob.receive_messages_async().await.unwrap();

    bob.decline_invite_async(&created.group_id).await.unwrap();

    let state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(state.members.len(), 1);
    assert!(state.pending_invites.is_empty(), "the decline reaches the server");
    let conversation_id = format!("group-{}", created.group_id);
    assert!(
        !bob.load_conversations_async()
            .await
            .unwrap()
            .iter()
            .any(|c| c.conversation_id == conversation_id),
        "a declined invite leaves the chat list"
    );
}

/// An invite from someone the invitee blocked is discarded outright.
#[tokio::test]
async fn blocked_inviter_invite_is_dropped() {
    let alice = account().await;
    let bob = account().await;
    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    bob.block_contact_async(&alice_did, true).await.unwrap();
    let created = alice.create_group_async("Spam group", "", 0).await.unwrap();
    alice.invite_member_async(&created.group_id, &bob_did, 0).await.unwrap();
    bob.receive_messages_async().await.unwrap();

    let conversation_id = format!("group-{}", created.group_id);
    assert!(
        !bob.load_conversations_async()
            .await
            .unwrap()
            .iter()
            .any(|c| c.conversation_id == conversation_id),
        "a blocked inviter's group never appears"
    );
    assert!(
        bob.accept_invite_async(&created.group_id).await.is_err(),
        "there is nothing to join"
    );
}

/// The live (WebSocket) receive path surfaces a stranger's invite as a
/// `GroupInvite { is_request: true }` event without joining.
#[tokio::test]
async fn live_stranger_invite_is_a_request_event() {
    let alice = Arc::new(account().await);
    let bob = Arc::new(account().await);
    alice.start_reconnect_task();
    bob.start_reconnect_task();
    let bob_did = bob.did_async().await;

    let created = alice.create_group_async("Live invite", "", 0).await.unwrap();
    alice.invite_member_async(&created.group_id, &bob_did, 0).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut seen = None;
    while seen.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "bob should see a GroupInvite event");
        let batch = tokio::time::timeout(remaining, bob.next_events_async())
            .await
            .expect("timed out waiting for the invite")
            .unwrap();
        seen = batch.into_iter().find_map(|ev| match ev {
            IncomingEvent::GroupInvite { group_id, is_request, .. }
                if group_id == created.group_id =>
            {
                Some(is_request)
            }
            _ => None,
        });
    }
    assert_eq!(seen, Some(true), "a stranger's invite is a request");

    let state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(state.members.len(), 1, "nothing was joined automatically");
}
