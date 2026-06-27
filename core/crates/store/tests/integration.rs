use crypto::{
    prekeys::{
        generate_kyber_prekey, generate_one_time_prekeys, generate_signed_prekey,
        RecipientKeyBundle,
    },
    session::{decrypt, encrypt, initiate_session, DeviceAddress},
    IdentityKeyPair,
};
use proptest::prelude::*;
use store::{
    account::DeviceAccount,
    DeviceStore,
};
use types::{AccountId, DeviceId, Timestamp};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// A fully initialised local peer ready for use in tests.
struct Peer {
    store: DeviceStore,
    identity: IdentityKeyPair,
    address: DeviceAddress,
    reg_id: u32,
}

impl Peer {
    async fn new(name: &str) -> Self {
        let store = DeviceStore::open_in_memory().await.expect("open in-memory store");
        let identity = IdentityKeyPair::generate();
        // libsignal registration IDs are arbitrary u32s assigned at account
        // creation and included in prekey bundles so recipients can detect
        // reregistration. Any non-zero value works for tests.
        let reg_id = 1u32;
        // keypair → identity.db; registration_id → device.db. The crypto path
        // reads registration_id via get_local_registration_id during session ops.
        store
            .save_identity_keypair(&identity)
            .await
            .expect("save identity keypair");
        store
            .save_device_account(&DeviceAccount {
                server_url: String::new(),
                device_id: 1,
                registered_at: Timestamp(0),
                registration_id: reg_id,
            })
            .await
            .expect("save device account");
        Peer {
            store,
            identity,
            address: DeviceAddress::new(AccountId::new(name), DeviceId::new(1)),
            reg_id,
        }
    }

    /// Generate and store a fresh prekey bundle, returning the wire format
    /// that would be published to the homeserver.
    async fn publish_bundle(&self, signed_id: u32, kyber_id: u32) -> RecipientKeyBundle {
        let signed = generate_signed_prekey(&self.identity, signed_id)
            .expect("generate signed prekey");
        self.store
            .save_signed_prekey(signed_id, &signed.record)
            .await
            .expect("save signed prekey");

        let one_time = generate_one_time_prekeys(1, 10).expect("generate one-time prekeys");
        let records: Vec<(u32, Vec<u8>)> = one_time
            .iter()
            .map(|pk| (pk.wire.id, pk.record.clone()))
            .collect();
        self.store
            .save_one_time_prekeys(&records)
            .await
            .expect("save one-time prekeys");

        let kyber = generate_kyber_prekey(&self.identity, kyber_id)
            .expect("generate kyber prekey");
        self.store
            .save_kyber_prekeys(&[(kyber_id, kyber.record.clone())])
            .await
            .expect("save kyber prekeys");

        RecipientKeyBundle {
            identity_key: self.identity.public_key().serialize(),
            registration_id: self.reg_id,
            device_id: 1,
            signed_prekey: signed.wire,
            one_time_prekey: Some(one_time[0].wire.clone()),
            kyber_prekey: kyber.wire,
        }
    }
}

/// Set up a session: Alice fetches Bob's bundle and initiates.
/// Returns (alice, bob) ready for message exchange.
async fn established_session() -> (Peer, Peer) {
    let mut alice = Peer::new("alice").await;
    let bob = Peer::new("bob").await;
    let bob_bundle = bob.publish_bundle(1, 1).await;
    initiate_session(&mut alice.store, &alice.address, &bob.address, &bob_bundle)
        .await
        .expect("initiate session");
    (alice, bob)
}

fn run<F: std::future::Future<Output = T>, T>(f: F) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

// ── Store unit tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn store_opens_and_migrates() {
    DeviceStore::open_in_memory().await.expect("store should open cleanly");
}

#[tokio::test]
async fn identity_round_trip() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    let keypair = IdentityKeyPair::generate();

    store.save_identity_keypair(&keypair).await.unwrap();

    let loaded = store.load_identity().await.unwrap().expect("identity should be present");
    assert_eq!(keypair.serialize(), loaded.serialize());
    assert_eq!(keypair.public_key().serialize(), loaded.public_key().serialize());
}

#[tokio::test]
async fn registration_round_trip() {
    let store = DeviceStore::open_in_memory().await.unwrap();

    // DID → identity.db, (server_url, device_id, registration_id) → device.db.
    assert!(store.load_did().await.unwrap().is_none());
    assert!(store.load_device_account().await.unwrap().is_none());

    store.save_did("did:plc:abc123", Timestamp::now()).await.unwrap();
    store
        .save_device_account(&DeviceAccount {
            server_url: "https://home.example.com".to_string(),
            device_id: 1,
            registered_at: Timestamp::now(),
            registration_id: 7,
        })
        .await
        .unwrap();

    let (did, _) = store.load_did().await.unwrap().expect("did should be present");
    let dev = store
        .load_device_account()
        .await
        .unwrap()
        .expect("device account should be present");
    assert_eq!(did, "did:plc:abc123");
    assert_eq!(dev.server_url, "https://home.example.com");
    assert_eq!(dev.device_id, 1);
    assert_eq!(dev.registration_id, 7);
}

#[tokio::test]
async fn prekey_pool_count() {
    let peer = Peer::new("alice").await;
    assert_eq!(peer.store.remaining_one_time_prekey_count().await.unwrap(), 0);
    assert_eq!(peer.store.remaining_kyber_prekey_count().await.unwrap(), 0);

    peer.publish_bundle(1, 1).await;

    assert_eq!(peer.store.remaining_one_time_prekey_count().await.unwrap(), 10);
    assert_eq!(peer.store.remaining_kyber_prekey_count().await.unwrap(), 1);
}

#[tokio::test]
async fn load_conversations_dedups_tied_timestamps() {
    // Regression (device-linking duplicate-group bug, docs/04 §4): a conversation
    // with several messages sharing the max sent_at must yield exactly ONE
    // conversation row. A freshly linked device receives bursts of group events /
    // SKDMs with identical (or zero) timestamps; the old MAX(sent_at) self-join
    // returned one row per tie, surfacing the same group many times in the list.
    use store::messages::HistoryMessage;

    let store = DeviceStore::open_in_memory().await.unwrap();
    let msg = |id: &str, ts: i64| HistoryMessage {
        id: id.to_string(),
        conversation_id: "group-G".to_string(),
        sender_did: "did:plc:alice".to_string(),
        body: "hi".to_string(),
        sent_at: Timestamp(ts),
        edited_at: None,
        read_at: None,
        delivery_status: 1,
        edit_count: 0,
        deleted_at: None,
        kind: 0,
        metadata: None,
        expire_timer_secs: 0,
        expire_at: None,
    };

    store.save_message(&msg("m1", 1000)).await.unwrap();
    store.save_message(&msg("m2", 1000)).await.unwrap();
    store.save_message(&msg("m3", 1000)).await.unwrap();
    // A second conversation, to prove dedup is per-conversation, not global.
    store.save_message(&msg2_other("dm-x", 1000)).await.unwrap();

    let convs = store.load_conversations(Timestamp(2000)).await.unwrap();
    let group_rows: Vec<_> = convs.iter().filter(|c| c.conversation_id == "group-G").collect();
    assert_eq!(
        group_rows.len(),
        1,
        "tied timestamps must collapse to one conversation row, got {group_rows:?}"
    );
    // Deterministic tie-break: highest id wins.
    assert_eq!(group_rows[0].last_message.as_ref().unwrap().id, "m3");
    assert_eq!(
        convs.iter().filter(|c| c.conversation_id == "dm-x").count(),
        1,
        "the unrelated conversation still appears exactly once"
    );
}

fn msg2_other(conv: &str, ts: i64) -> store::messages::HistoryMessage {
    store::messages::HistoryMessage {
        id: format!("{conv}-1"),
        conversation_id: conv.to_string(),
        sender_did: "did:plc:bob".to_string(),
        body: "yo".to_string(),
        sent_at: Timestamp(ts),
        edited_at: None,
        read_at: None,
        delivery_status: 1,
        edit_count: 0,
        deleted_at: None,
        kind: 0,
        metadata: None,
        expire_timer_secs: 0,
        expire_at: None,
    }
}

#[tokio::test]
async fn message_queue_enqueue_drain_deliver() {
    use store::messages::QueuedMessage;
    use types::MessageId;

    let store = DeviceStore::open_in_memory().await.unwrap();

    let msg = QueuedMessage {
        id: MessageId::new(),
        recipient_name: "bob".to_string(),
        recipient_device_id: 1,
        ciphertext: vec![1, 2, 3],
        message_kind: 1,
        enqueued_at: Timestamp::now(),
    };

    assert!(store.drain().await.unwrap().is_empty());

    store.enqueue(&msg).await.unwrap();
    let queued = store.drain().await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].ciphertext, msg.ciphertext);

    store.mark_delivered(msg.id).await.unwrap();
    assert!(store.drain().await.unwrap().is_empty());
}

// ── Session round-trip tests ──────────────────────────────────────────────────

#[tokio::test]
async fn session_alice_to_bob_round_trip() {
    let (mut alice, mut bob) = established_session().await;
    let plaintext = b"hello, Bob!";

    let encrypted = encrypt(&mut alice.store, &alice.address, &bob.address, plaintext)
        .await
        .expect("encrypt");
    let decrypted = decrypt(&mut bob.store, &bob.address, &alice.address, &encrypted)
        .await
        .expect("decrypt");

    assert_eq!(decrypted, plaintext);
}

#[tokio::test]
async fn first_message_is_prekey_type() {
    use crypto::session::MessageKind;
    let (mut alice, _bob) = established_session().await;

    let encrypted = encrypt(&mut alice.store, &alice.address, &bob_addr(), b"hi")
        .await
        .expect("encrypt");

    assert_eq!(
        encrypted.kind,
        MessageKind::PreKey,
        "first message to a new session should be a PreKey message"
    );
}

fn bob_addr() -> DeviceAddress {
    DeviceAddress::new(AccountId::new("bob"), DeviceId::new(1))
}

#[tokio::test]
async fn ratchet_advances_across_multiple_messages() {
    let (mut alice, mut bob) = established_session().await;

    for i in 0u8..10 {
        let plaintext = format!("message {i}").into_bytes();
        let enc = encrypt(&mut alice.store, &alice.address, &bob.address, &plaintext)
            .await
            .expect("encrypt");
        let dec = decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
            .await
            .expect("decrypt");
        assert_eq!(dec, plaintext, "message {i} should round-trip correctly");
    }
}

#[tokio::test]
async fn bidirectional_messages() {
    let (mut alice, mut bob) = established_session().await;

    // Alice sends first (establishes Bob's inbound session)
    let enc = encrypt(&mut alice.store, &alice.address, &bob.address, b"hello Bob")
        .await
        .unwrap();
    decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
        .await
        .unwrap();

    // Now alternate directions
    for i in 0u8..5 {
        let msg = format!("alice→bob {i}").into_bytes();
        let enc = encrypt(&mut alice.store, &alice.address, &bob.address, &msg)
            .await
            .unwrap();
        let dec = decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
            .await
            .unwrap();
        assert_eq!(dec, msg);

        let msg = format!("bob→alice {i}").into_bytes();
        let enc = encrypt(&mut bob.store, &bob.address, &alice.address, &msg)
            .await
            .unwrap();
        let dec = decrypt(&mut alice.store, &alice.address, &bob.address, &enc)
            .await
            .unwrap();
        assert_eq!(dec, msg);
    }
}

#[tokio::test]
async fn messages_become_whisper_after_bob_replies() {
    use crypto::session::MessageKind;

    let (mut alice, mut bob) = established_session().await;

    // Alice's messages remain PreKey until Bob replies — the session is
    // "unacknowledged" until Alice receives a message from Bob.
    let enc1 = encrypt(&mut alice.store, &alice.address, &bob.address, b"first")
        .await
        .unwrap();
    assert_eq!(enc1.kind, MessageKind::PreKey);

    let enc2 = encrypt(&mut alice.store, &alice.address, &bob.address, b"second")
        .await
        .unwrap();
    assert_eq!(enc2.kind, MessageKind::PreKey, "still PreKey — no reply from Bob yet");

    // Bob decrypts both
    decrypt(&mut bob.store, &bob.address, &alice.address, &enc1).await.unwrap();
    decrypt(&mut bob.store, &bob.address, &alice.address, &enc2).await.unwrap();

    // Bob replies — this acknowledges the session on Alice's side
    let reply = encrypt(&mut bob.store, &bob.address, &alice.address, b"hey Alice")
        .await
        .unwrap();
    decrypt(&mut alice.store, &alice.address, &bob.address, &reply).await.unwrap();

    // Now Alice's messages are Whisper
    let enc3 = encrypt(&mut alice.store, &alice.address, &bob.address, b"third")
        .await
        .unwrap();
    assert_eq!(enc3.kind, MessageKind::Whisper, "PreKey→Whisper after Bob's reply");
}

#[tokio::test]
async fn prekey_consumed_after_session_init() {
    let (mut alice, mut bob) = established_session().await;
    assert_eq!(bob.store.remaining_one_time_prekey_count().await.unwrap(), 10);

    // Alice sends first message; Bob decrypts, consuming the one-time prekey
    let enc = encrypt(&mut alice.store, &alice.address, &bob.address, b"hi")
        .await
        .unwrap();
    decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
        .await
        .unwrap();

    assert_eq!(
        bob.store.remaining_one_time_prekey_count().await.unwrap(),
        9,
        "one-time prekey should be consumed after session initiation"
    );
}

// ── Property-based tests ──────────────────────────────────────────────────────

/// A message in the generated sequence: direction (true = Alice→Bob) and payload.
#[derive(Debug, Clone)]
struct TestMessage {
    alice_to_bob: bool,
    payload: Vec<u8>,
}

fn arb_message() -> impl Strategy<Value = TestMessage> {
    (any::<bool>(), prop::collection::vec(any::<u8>(), 1..64)).prop_map(
        |(alice_to_bob, payload)| TestMessage { alice_to_bob, payload },
    )
}

proptest! {
    /// Any sequence of sends and receives should leave both sessions in a
    /// consistent state: every message decrypts to its original plaintext.
    #[test]
    fn any_message_sequence_round_trips(
        messages in prop::collection::vec(arb_message(), 1..20)
    ) {
        let result: Result<(), String> = run(async move {
            let (mut alice, mut bob) = established_session().await;

            // Bootstrap: Alice→Bob first to give Bob an inbound session.
            let enc = encrypt(&mut alice.store, &alice.address, &bob.address, b"bootstrap")
                .await
                .map_err(|e| e.to_string())?;
            decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
                .await
                .map_err(|e| e.to_string())?;

            for msg in &messages {
                if msg.alice_to_bob {
                    let enc =
                        encrypt(&mut alice.store, &alice.address, &bob.address, &msg.payload)
                            .await
                            .map_err(|e| e.to_string())?;
                    let dec = decrypt(&mut bob.store, &bob.address, &alice.address, &enc)
                        .await
                        .map_err(|e| e.to_string())?;
                    if dec != msg.payload {
                        return Err(format!(
                            "Alice→Bob: decrypted {:?} != original {:?}",
                            dec, msg.payload
                        ));
                    }
                } else {
                    let enc =
                        encrypt(&mut bob.store, &bob.address, &alice.address, &msg.payload)
                            .await
                            .map_err(|e| e.to_string())?;
                    let dec = decrypt(&mut alice.store, &alice.address, &bob.address, &enc)
                        .await
                        .map_err(|e| e.to_string())?;
                    if dec != msg.payload {
                        return Err(format!(
                            "Bob→Alice: decrypted {:?} != original {:?}",
                            dec, msg.payload
                        ));
                    }
                }
            }
            Ok(())
        });

        prop_assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}

#[tokio::test]
async fn sender_key_shared_is_tracked_per_device() {
    // docs/04 multi-device groups: SKDM distribution is tracked per
    // (group, recipient_did, device) so a co-member linking a new device is
    // detected as "owes an SKDM" even though the DID already had the key.
    let store = DeviceStore::open_in_memory().await.unwrap();
    let g = "group-skdm";
    let bob = "did:plc:bob";

    // Nothing shared yet → every queried device is owed the key.
    assert_eq!(
        store.sender_key_unshared_devices(g, bob, &[1, 2]).await.unwrap(),
        vec![1, 2]
    );

    // Share with device 1; device 2 still owes.
    store.mark_sender_key_shared(g, bob, 1).await.unwrap();
    assert_eq!(store.sender_key_unshared_devices(g, bob, &[1, 2]).await.unwrap(), vec![2]);

    // Bulk-mark 2 and 3; now none of {1,2,3} owes.
    store.mark_sender_key_shared_devices(g, bob, &[2, 3]).await.unwrap();
    assert!(store.sender_key_unshared_devices(g, bob, &[1, 2, 3]).await.unwrap().is_empty());

    // Bob links a brand-new device 4 → detected as owed (the multi-device bug).
    assert_eq!(store.sender_key_unshared_devices(g, bob, &[1, 2, 3, 4]).await.unwrap(), vec![4]);

    // Tracking is scoped per (group, recipient): a different group/recipient is
    // untouched by the marks above.
    assert_eq!(store.sender_key_unshared_devices("group-other", bob, &[1]).await.unwrap(), vec![1]);
    assert_eq!(store.sender_key_unshared_devices(g, "did:plc:carol", &[1]).await.unwrap(), vec![1]);

    // Re-seeding clears the whole group so every device re-receives the key.
    store.clear_sender_key_shared(g).await.unwrap();
    assert_eq!(store.sender_key_unshared_devices(g, bob, &[1, 2]).await.unwrap(), vec![1, 2]);
}
