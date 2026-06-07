//! End-to-end test for **live** group-message delivery over the WebSocket.
//!
//! Mirrors what the iOS app and adminbot actually do at runtime — both clients
//! run a reconnect task, the server pushes group fan-outs through it, and
//! receivers drain via `next_events_async`. The existing `e2e_groups.rs`
//! tests use the explicit `fetch_group_messages` poll path and would pass
//! even if WS push were entirely broken; this file closes that gap.
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

use app_core::{AppCore, IncomingEvent};
use std::sync::Arc;
use std::time::Duration;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::Store {
    let store = store::Store::open_in_memory().await.unwrap();
    store.migrate().await.unwrap();
    store
}

/// Create a bot account and start its reconnect task. Mirrors production
/// FFI constructors: returns an `Arc<AppCore>` with a running WS loop.
async fn live_account(url: &str) -> Arc<AppCore> {
    let core = AppCore::create_account_with_store(url, test_store().await, None, true)
        .await
        .unwrap();
    let arc = Arc::new(core);
    arc.start_reconnect_task();
    arc
}

/// Drain `next_events_async` until we see an event matching `pred`, or
/// until `timeout` elapses. Returns the matched event.
async fn wait_for_event(
    core: &AppCore,
    pred: impl Fn(&IncomingEvent) -> bool,
    timeout: Duration,
) -> Option<IncomingEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let batch = match tokio::time::timeout(remaining, core.next_events_async()).await {
            Ok(Ok(b)) => b,
            Ok(Err(_)) | Err(_) => return None,
        };
        for ev in batch {
            if pred(&ev) {
                return Some(ev);
            }
        }
    }
}

/// Group message arrives at every member's `next_events` channel via WS
/// push — no explicit `fetch_group_messages` call required. Regression
/// guard for the missing `subscribe_group_pseudonyms` wiring and the
/// missing `next_group_message` arm in the receive loop.
#[tokio::test]
async fn group_send_arrives_via_ws_push() {
    let url = server_url();

    let alice = live_account(&url).await;
    let bob = live_account(&url).await;
    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // 1. Alice creates the group and invites Bob.
    let created = alice
        .create_group_async("ws-push", "live delivery", 0)
        .await
        .unwrap();
    alice
        .invite_member_async(&created.group_id, &bob_did, 0)
        .await
        .unwrap();

    // 2. Bob's reconnect task receives the GroupContext DM, auto-accepts the
    //    invite, and subscribes to the new group_push_pseudonym. We surface a
    //    typed `GroupInvite` event when this completes — block on it.
    let invite = wait_for_event(
        &bob,
        |ev| matches!(ev, IncomingEvent::GroupInvite { group_id, .. } if group_id == &created.group_id),
        Duration::from_secs(5),
    )
    .await;
    assert!(
        invite.is_some(),
        "bob should see a GroupInvite event for {}",
        created.group_id
    );

    // 3. Refresh Alice's cached group state so she sees Bob in `members`
    //    (instead of her stale optimistic state where he's still in
    //    `pending_invites`). Without this, `send_group_message` filters Bob
    //    out of the recipient set and the send is a no-op.
    //
    //    TODO: this exposes a real product bug — Alice has no live signal
    //    that Bob accepted, so a UI sending right after invite would silently
    //    drop the message. Either auto-refresh in `send_group_message` when
    //    pending invites exist, or push a notification through the bot loop.
    alice
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();

    // 4. Alice sends into the group. With Bob now a full member and his WS
    //    subscribed, the server should fan this out live.
    let payload = b"hello group, via ws push";
    alice
        .send_group_message_async(&created.group_id, payload)
        .await
        .unwrap();

    // 4. Bob receives the group message through `next_events_async`. No
    //    `fetch_group_messages` call.
    let alice_did_check = alice_did.clone();
    let group_id_check = created.group_id.clone();
    let got = wait_for_event(
        &bob,
        |ev| match ev {
            IncomingEvent::Message { msg } => {
                msg.sender_did == alice_did_check
                    && msg.group_id.as_deref() == Some(group_id_check.as_str())
                    && msg.plaintext == payload
            }
            _ => false,
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        got.is_some(),
        "bob should have received alice's group message via WS push within 5s"
    );

    // 5. Symmetric direction: Bob → Alice. Catches the "founder didn't
    //    subscribe to its own pseudonym after create_group" failure mode.
    let reply = b"got it";
    bob.send_group_message_async(&created.group_id, reply)
        .await
        .unwrap();
    let got = wait_for_event(
        &alice,
        |ev| match ev {
            IncomingEvent::Message { msg } => {
                msg.sender_did == bob_did
                    && msg.group_id.as_deref() == Some(created.group_id.as_str())
                    && msg.plaintext == reply
            }
            _ => false,
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        got.is_some(),
        "alice should have received bob's group reply via WS push within 5s"
    );
}
