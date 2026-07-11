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

mod common;

use app_core::AppCore;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

#[tokio::test]
async fn create_invite_accept_promote_remove_roundtrip() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
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

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let carol = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
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

/// A later joiner can decrypt an earlier member's group messages, even though
/// that earlier member's join-time SKDM never reached them.
///
/// Topology: alice invites bob and carol. Bob accepts *first* — his accept-time
/// SKDM fans out only to the members present then (alice), not carol. Carol
/// accepts *second*. So carol has alice's key (from the invite) but never
/// received bob's. Before Signal-style lazy distribution, bob's group message
/// was undecryptable for carol ("missing sender key state"). Now bob's send
/// first ships his SKDM to any member that lacks it (carol), so she decrypts.
#[tokio::test]
async fn later_joiner_decrypts_earlier_members_messages() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let carol = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();

    let bob_did = bob.did_async().await;
    let carol_did = carol.did_async().await;

    let created = alice
        .create_group_async("Trio", "lazy-skdm e2e", 0)
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

    // Bob accepts first (his SKDM reaches only alice), carol accepts second.
    let _ = bob.receive_messages_async().await.unwrap();
    let _ = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    bob.accept_invite_async(&created.group_id).await.unwrap();
    let _ = carol.receive_messages_async().await.unwrap();
    let _ = carol
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    carol.accept_invite_async(&created.group_id).await.unwrap();

    // Alice drains both accept-time SKDMs. Carol deliberately does NOT receive
    // bob's key here — bob accepted before carol was a member, so it was never
    // sent to her.
    let _ = alice.receive_messages_async().await.unwrap();

    // Bob refreshes group state so his cached membership includes carol (who
    // joined after him); the send + distribution work off this cache.
    let bob_state = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(bob_state.members.len(), 3);

    // Bob sends to the group. The send path must first distribute bob's sender
    // key to carol (the member who lacks it).
    let plaintext = b"bob speaking to a group carol joined late";
    bob.send_group_message_async(&created.group_id, plaintext)
        .await
        .unwrap();

    // Carol drains the lazily-distributed SKDM DM, then the group message.
    let _ = carol.receive_messages_async().await.unwrap();
    let carol_msgs = carol
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(
        carol_msgs.len(),
        1,
        "carol must decrypt bob's message via the lazily-distributed sender key"
    );
    assert_eq!(carol_msgs[0].plaintext, plaintext);
    assert_eq!(carol_msgs[0].sender_did, bob_did);

    // And a second message from bob needs no further distribution (carol is now
    // marked shared) — it still decrypts.
    let plaintext2 = b"second message, no resend needed";
    bob.send_group_message_async(&created.group_id, plaintext2)
        .await
        .unwrap();
    let _ = carol.receive_messages_async().await.unwrap();
    let carol_msgs2 = carol
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(carol_msgs2.len(), 1);
    assert_eq!(carol_msgs2[0].plaintext, plaintext2);
}

/// A member can group-send even to a peer it has never exchanged a DM with:
/// the send path establishes the missing Double Ratchet session on the fly
/// (X3DH from a fetched prekey bundle) instead of aborting with `NoSession`.
///
/// Bob joins while he is the only invitee, so his accept-time SKDM fan-out
/// reaches only alice. Carol joins afterwards, and bob never drains carol's
/// SKDM DM — so bob's store has no session with carol. Before lazy
/// establishment, bob's group send would fail outright; now it must go
/// through, and alice (a recipient bob was already set up for) decrypts it.
#[tokio::test]
async fn group_send_establishes_missing_session() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let carol = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();

    let bob_did = bob.did_async().await;
    let carol_did = carol.did_async().await;

    let created = alice
        .create_group_async("Trio", "lazy-session e2e", 0)
        .await
        .unwrap();

    // Bob joins first, while he is the only invitee.
    alice
        .invite_member_async(&created.group_id, &bob_did, 0)
        .await
        .unwrap();
    let _ = bob.receive_messages_async().await.unwrap();
    let _ = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    bob.accept_invite_async(&created.group_id).await.unwrap();

    // Carol joins afterwards. Bob deliberately never drains carol's accept-time
    // SKDM DM, so bob's store holds no session with carol.
    alice
        .invite_member_async(&created.group_id, &carol_did, 0)
        .await
        .unwrap();
    let _ = carol.receive_messages_async().await.unwrap();
    let _ = carol
        .fetch_group_state_async(&created.group_id)
        .await
        .unwrap();
    carol.accept_invite_async(&created.group_id).await.unwrap();

    // Alice drains both invitees' SKDMs so she can decrypt their group messages.
    let _ = alice.receive_messages_async().await.unwrap();

    // Bob refreshes state and now sees carol as a member, but has no session
    // with her. The send must establish one on the fly rather than fail.
    let bob_state = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(bob_state.members.len(), 3);

    let plaintext = b"bob to the group";
    bob.send_group_message_async(&created.group_id, plaintext)
        .await
        .expect("group send should establish the missing carol session, not fail with NoSession");

    // Alice receives and decrypts — proving the fan-out actually went out
    // instead of erroring midway on the missing carol session.
    let alice_msgs = alice
        .fetch_group_messages_async(&created.group_id)
        .await
        .unwrap();
    assert_eq!(alice_msgs.len(), 1);
    assert_eq!(alice_msgs[0].plaintext, plaintext);
    assert_eq!(alice_msgs[0].sender_did, bob_did);
}

/// Missing-sender-key recovery: a group message that arrives before the
/// sender's Sender Key is installed is buffered locally (not dropped), and
/// recovered automatically when that sender's SKDM is finally processed.
///
/// Ordering is forced by the two transports: group content is pulled via
/// `fetch_group_messages` while SKDMs arrive as pairwise DMs drained by
/// `receive_messages`. Alice pulls bob's group message *before* draining his
/// accept-time SKDM, so she has no key for it yet.
#[tokio::test]
async fn buffered_group_message_recovers_when_skdm_arrives() {
    let url = server_url();

    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob_did = bob.did_async().await;

    // Group with alice + bob; bob accepts (his accept DMs an SKDM to alice).
    let created = alice
        .create_group_async("Recovery", "buffer+retry e2e", 0)
        .await
        .unwrap();
    alice
        .invite_member_async(&created.group_id, &bob_did, 0)
        .await
        .unwrap();
    let _ = bob.receive_messages_async().await.unwrap();
    let _ = bob.fetch_group_state_async(&created.group_id).await.unwrap();
    bob.accept_invite_async(&created.group_id).await.unwrap();

    // Alice sees bob as a member but DELIBERATELY does not drain his SKDM yet,
    // so she still lacks his Sender Key.
    let alice_state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(alice_state.members.len(), 2);

    // Bob sends a group message; alice pulls it before she has his key.
    let plaintext = b"hi alice, freshly-joined bob here";
    bob.send_group_message_async(&created.group_id, plaintext)
        .await
        .unwrap();

    // Alice's group pull can't decrypt it (no Sender Key) -> buffered locally,
    // NOT surfaced and NOT lost.
    let pulled = alice.fetch_group_messages_async(&created.group_id).await.unwrap();
    assert!(
        pulled.is_empty(),
        "message must be buffered (not surfaced) while bob's key is missing"
    );

    // Alice drains bob's SKDM. Processing it installs his key and retries the
    // buffered message, which surfaces in the receive_messages result.
    let recovered = alice.receive_messages_async().await.unwrap();
    let mine: Vec<_> = recovered
        .into_iter()
        .filter(|m| m.group_id.as_deref() == Some(created.group_id.as_str()))
        .collect();
    assert_eq!(mine.len(), 1, "the buffered group message should be recovered");
    assert_eq!(mine[0].plaintext, plaintext);
    assert_eq!(mine[0].sender_did, bob_did);

    // Idempotent: nothing is re-surfaced or duplicated on subsequent drains
    // (the buffered row was deleted, the server row already acked).
    let pulled2 = alice.fetch_group_messages_async(&created.group_id).await.unwrap();
    assert!(pulled2.is_empty());
    let drained2 = alice.receive_messages_async().await.unwrap();
    assert!(
        drained2
            .iter()
            .all(|m| m.group_id.as_deref() != Some(created.group_id.as_str())),
        "recovered message must not surface twice"
    );
}
