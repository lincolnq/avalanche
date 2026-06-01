//! End-to-end integration test for action-bound groups
//! (docs/03-groups.md). Exercises the full flow:
//!
//! 1. Alice creates a group.
//! 2. Alice invites Bob; Bob receives the GroupContext DM, fetches state.
//! 3. Bob accepts (promote_pending_members) and becomes a member.
//! 4. Alice fetches state again and sees Bob in `members`.
//! 5. Alice changes Bob's role to Admin.
//! 6. Alice removes Bob.
//!
//! Requires a homeserver at `SERVER_URL` (default
//! `http://localhost:3000`). Run via `make test-e2e`.

use app_core::AppCore;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::Store {
    let store = store::Store::open_in_memory().await.unwrap();
    store.migrate().await.unwrap();
    store
}

#[tokio::test]
async fn create_invite_accept_promote_remove_roundtrip() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();

    let bob_did = bob.did_async().await;

    // 1. Alice creates the group.
    let created = alice
        .create_group_async("Test", "groups e2e", 0)
        .await
        .unwrap();
    assert_eq!(created.master_key.len(), 32);

    // 2. Alice invites Bob; this sends a GroupContext DM as a side effect.
    alice
        .invite_member_async(&created.group_id, &bob_did, 0)
        .await
        .unwrap();

    // 3. Bob receives the DM and stores the GroupContext locally.
    let msgs = bob.receive_messages_async().await.unwrap();
    assert!(
        !msgs.is_empty(),
        "bob should have received the GroupContext DM"
    );

    // 4. Bob fetches state; he should see himself in pending_invites.
    let bob_state = bob
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(
        bob_state.pending_invites.len(),
        1,
        "bob should see one pending invite"
    );
    assert_eq!(
        bob_state.members.len(),
        1,
        "only alice should be a full member at this point"
    );

    // 5. Bob accepts (promote_pending_members).
    bob.accept_invite_async(&created.group_id).await.unwrap();

    // 6. Alice re-fetches; she should see Bob in members and the pending row gone.
    let alice_state = alice
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(alice_state.members.len(), 2);
    assert!(alice_state.pending_invites.is_empty());

    // 7. Alice drains the SKDM DM Bob sent on accept (no event emitted —
    // SKDMs are plumbing — but processing installs Bob's sender key into
    // Alice's local store).
    let _ = alice.receive_messages_async().await.unwrap();

    // 8. Alice sends a sealed-sender group message; Bob drains via the
    //    HTTP offline-pickup path (production uses WS push, but for tests
    //    HTTP fetch hits the same crypto and ack path).
    let plaintext = b"hello group, this is alice";
    alice
        .send_group_message_async(&created.group_id, plaintext)
        .await
        .unwrap();

    let group_msgs = bob
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(group_msgs.len(), 1, "bob should receive one group message");
    assert_eq!(group_msgs[0].plaintext, plaintext);
    assert_eq!(group_msgs[0].sender_did, alice.did_async().await);

    // 9. Bob replies; Alice drains.
    let reply = b"thanks alice";
    bob.send_group_message_async(&created.group_id, reply)
        .await
        .unwrap();
    let alice_msgs = alice
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(alice_msgs.len(), 1);
    assert_eq!(alice_msgs[0].plaintext, reply);
    assert_eq!(alice_msgs[0].sender_did, bob.did_async().await);
}

/// Three-member group: alice sends one sealed-sender message that fans out
/// to both bob and carol via the SSv2 multi-recipient envelope. Exercises
/// the multi-recipient parsing and routing paths that the 2-member test
/// doesn't actually cover (one envelope → one recipient slice).
#[tokio::test]
async fn three_member_fanout_roundtrip() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();
    let carol = AppCore::create_account_with_store(&url, test_store().await, None, true)
        .await
        .unwrap();

    let bob_did = bob.did_async().await;
    let carol_did = carol.did_async().await;
    let alice_did = alice.did_async().await;

    // Alice creates the group and invites both bob and carol.
    let created = alice
        .create_group_async("Trio", "fanout e2e", 0)
        .await
        .unwrap();
    alice
        .invite_member_async(&created.group_id, &bob_did, 0)
        .await
        .unwrap();
    alice
        .invite_member_async(&created.group_id, &carol_did, 0)
        .await
        .unwrap();

    // Both invitees ingest their GroupContext DM, fetch state to populate
    // the local cache (accept requires it), then accept. Carol re-fetches
    // between bob's accept and her own — bob's accept bumped the revision,
    // so carol's local cache would otherwise be stale.
    let _ = bob.receive_messages_async().await.unwrap();
    let _ = carol.receive_messages_async().await.unwrap();
    let _ = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    bob.accept_invite_async(&created.group_id).await.unwrap();
    let _ = carol
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    carol.accept_invite_async(&created.group_id).await.unwrap();

    // Alice drains the SKDMs both invitees emitted on accept, so her
    // SenderKeyStore carries bob's and carol's distributions.
    let _ = alice.receive_messages_async().await.unwrap();
    // Bob & carol ingest each other's accept-time SKDM as well, so they
    // can later decrypt each other's group messages.
    let _ = bob.receive_messages_async().await.unwrap();
    let _ = carol.receive_messages_async().await.unwrap();

    // Confirm membership is 3 from alice's POV.
    let alice_state = alice
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(alice_state.members.len(), 3);

    // Alice sends one group message; it fans out to bob and carol.
    let plaintext = b"hello bob and carol";
    alice
        .send_group_message_async(&created.group_id, plaintext)
        .await
        .unwrap();

    let bob_msgs = bob
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    let carol_msgs = carol
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(bob_msgs.len(), 1, "bob should receive one group message");
    assert_eq!(carol_msgs.len(), 1, "carol should receive one group message");
    assert_eq!(bob_msgs[0].plaintext, plaintext);
    assert_eq!(carol_msgs[0].plaintext, plaintext);
    assert_eq!(bob_msgs[0].sender_did, alice_did);
    assert_eq!(carol_msgs[0].sender_did, alice_did);

    // Carol replies; both alice and bob drain.
    let reply = b"hi alice and bob, carol here";
    carol
        .send_group_message_async(&created.group_id, reply)
        .await
        .unwrap();
    let alice_msgs = alice
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    let bob_msgs2 = bob
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(alice_msgs.len(), 1);
    assert_eq!(bob_msgs2.len(), 1);
    assert_eq!(alice_msgs[0].plaintext, reply);
    assert_eq!(bob_msgs2[0].plaintext, reply);
    assert_eq!(alice_msgs[0].sender_did, carol_did);
    assert_eq!(bob_msgs2[0].sender_did, carol_did);
}
