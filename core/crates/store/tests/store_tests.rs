//! Tests that complement `integration.rs`. These cover store edge cases
//! not exercised by the end-to-end session tests.

use store::DeviceStore;

#[tokio::test]
async fn load_identity_returns_none_before_creation() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    assert!(store.load_identity().await.unwrap().is_none());
}

#[tokio::test]
async fn store_satisfies_crypto_store_trait() {
    use crypto::Store as CryptoStore;

    let store = DeviceStore::open_in_memory().await.unwrap();
    let identity = crypto::IdentityKeyPair::generate();
    store.save_identity_keypair(&identity).await.unwrap();

    fn assert_is_crypto_store(_s: &impl CryptoStore) {}
    assert_is_crypto_store(&store);
}

#[tokio::test]
async fn is_trusted_identity_is_direction_aware() {
    use libsignal_protocol::{Direction, DeviceId, IdentityKey, IdentityKeyStore, ProtocolAddress};

    // Build libsignal IdentityKeys via the crypto newtype's public bytes.
    fn fresh_key() -> IdentityKey {
        IdentityKey::decode(&crypto::IdentityKeyPair::generate().public_key().serialize()).unwrap()
    }

    let mut store = DeviceStore::open_in_memory().await.unwrap();
    let addr = ProtocolAddress::new("peer-uuid".to_string(), DeviceId::try_from(1u32).unwrap());

    let original = fresh_key();
    let rotated = fresh_key();

    // First contact (trust-on-first-use): trusted, then recorded.
    assert!(store
        .is_trusted_identity(&addr, &original, Direction::Receiving)
        .await
        .unwrap());
    IdentityKeyStore::save_identity(&mut store, &addr, &original)
        .await
        .unwrap();

    // Same key: trusted.
    assert!(store
        .is_trusted_identity(&addr, &original, Direction::Receiving)
        .await
        .unwrap());

    // Changed key (peer re-registered): trusted for Sending so a send can
    // re-establish, but rejected for Receiving so inbound attribution stays strict.
    assert!(
        store
            .is_trusted_identity(&addr, &rotated, Direction::Sending)
            .await
            .unwrap(),
        "a send must auto-accept a re-registered peer's new key"
    );
    assert!(
        !store
            .is_trusted_identity(&addr, &rotated, Direction::Receiving)
            .await
            .unwrap(),
        "an inbound message with a changed key must not be silently trusted"
    );
}

#[tokio::test]
async fn message_queue_ordering() {
    use store::messages::QueuedMessage;
    use types::{MessageId, Timestamp};

    let store = DeviceStore::open_in_memory().await.unwrap();

    // Enqueue out of chronological order.
    let msg_later = QueuedMessage {
        id: MessageId::new(),
        recipient_name: "bob".to_string(),
        recipient_device_id: 1,
        ciphertext: vec![2],
        message_kind: 0,
        enqueued_at: Timestamp(2000),
    };
    let msg_earlier = QueuedMessage {
        id: MessageId::new(),
        recipient_name: "carol".to_string(),
        recipient_device_id: 1,
        ciphertext: vec![1],
        message_kind: 0,
        enqueued_at: Timestamp(1000),
    };

    store.enqueue(&msg_later).await.unwrap();
    store.enqueue(&msg_earlier).await.unwrap();

    let drained = store.drain().await.unwrap();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].recipient_name, "carol", "earlier message should come first");
    assert_eq!(drained[1].recipient_name, "bob");
}

#[tokio::test]
async fn own_profile_round_trip() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    assert!(store.load_own_profile().await.unwrap().is_none());

    let profile = store::profiles::OwnProfile {
        profile_key: vec![7u8; 32],
        display_name: "Alice".into(),
    };
    store.save_own_profile(&profile).await.unwrap();

    let loaded = store.load_own_profile().await.unwrap().unwrap();
    assert_eq!(loaded.profile_key, vec![7u8; 32]);
    assert_eq!(loaded.display_name, "Alice");

    store.update_own_display_name("Alice Updated").await.unwrap();
    let loaded = store.load_own_profile().await.unwrap().unwrap();
    assert_eq!(loaded.display_name, "Alice Updated");
    assert_eq!(loaded.profile_key, vec![7u8; 32], "key unchanged on rename");
}

#[tokio::test]
async fn load_conversations_one_row_per_convo_newest_first() {
    use store::messages::HistoryMessage;
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    // Two messages in convA (newest at t=1000), one in convB (t=500).
    for (id, conv, sent_at, body) in [
        ("a1", "convA", 100i64, "older A"),
        ("a2", "convA", 1000i64, "newest A"),
        ("b1", "convB", 500i64, "only B"),
    ] {
        store.save_message(&HistoryMessage {
            id: id.into(),
            conversation_id: conv.into(),
            sender_did: "did:plc:bob".into(),
            body: body.into(),
            sent_at: Timestamp(sent_at),
            edited_at: None,
            read_at: None,
            delivery_status: 1,
            edit_count: 0,
            deleted_at: None,
            kind: 0,
            metadata: None,
            expire_timer_secs: 0,
            expire_at: None,
        }).await.unwrap();
    }

    let convs = store.load_conversations(Timestamp(2000)).await.unwrap();
    assert_eq!(convs.len(), 2, "one row per distinct conversation_id");
    assert_eq!(convs[0].conversation_id, "convA");
    assert_eq!(convs[0].last_message.as_ref().unwrap().body, "newest A");
    assert_eq!(convs[1].conversation_id, "convB");
    assert_eq!(convs[1].last_message.as_ref().unwrap().body, "only B");
}

#[tokio::test]
async fn contact_profile_cache() {
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    let did = "did:plc:bob000000000000000001";
    assert!(store.load_contact_profile(did).await.unwrap().is_none());

    let p = store::profiles::ContactProfile {
        did: did.into(),
        display_name: "Bob".into(),
        profile_key: vec![9u8; 32],
        fetched_at: Timestamp(1000),
    };
    store.upsert_contact_profile(&p).await.unwrap();

    let loaded = store.load_contact_profile(did).await.unwrap().unwrap();
    assert_eq!(loaded.display_name, "Bob");
    assert_eq!(loaded.profile_key, vec![9u8; 32]);

    let key = store.load_contact_profile_key(did).await.unwrap().unwrap();
    assert_eq!(key, vec![9u8; 32]);

    let p2 = store::profiles::ContactProfile {
        did: did.into(),
        display_name: "Bob (renamed)".into(),
        profile_key: vec![9u8; 32],
        fetched_at: Timestamp(2000),
    };
    store.upsert_contact_profile(&p2).await.unwrap();
    let loaded = store.load_contact_profile(did).await.unwrap().unwrap();
    assert_eq!(loaded.display_name, "Bob (renamed)");
    assert_eq!(loaded.fetched_at, Timestamp(2000));
}

#[tokio::test]
async fn account_info_cache_round_trip() {
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    let did = "did:local:adminbot";
    assert!(store.load_account_info(did).await.unwrap().is_none());

    let info = store::profiles::AccountInfoCache {
        did: did.into(),
        display_name: "Admin Bot".into(),
        is_bot: true,
        fetched_at: Timestamp(1000),
    };
    store.upsert_account_info(&info).await.unwrap();

    let loaded = store.load_account_info(did).await.unwrap().unwrap();
    assert_eq!(loaded.display_name, "Admin Bot");
    assert!(loaded.is_bot);
    assert_eq!(loaded.fetched_at, Timestamp(1000));

    // Re-fetch overwrites name + timestamp (server is authoritative).
    let info2 = store::profiles::AccountInfoCache {
        did: did.into(),
        display_name: "Admin Bot v2".into(),
        is_bot: true,
        fetched_at: Timestamp(2000),
    };
    store.upsert_account_info(&info2).await.unwrap();
    let loaded = store.load_account_info(did).await.unwrap().unwrap();
    assert_eq!(loaded.display_name, "Admin Bot v2");
    assert_eq!(loaded.fetched_at, Timestamp(2000));
}

#[tokio::test]
async fn list_bot_dids_returns_only_bots() {
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    assert!(store.list_bot_dids().await.unwrap().is_empty());

    let bot = store::profiles::AccountInfoCache {
        did: "did:local:adminbot".into(),
        display_name: "Admin Bot".into(),
        is_bot: true,
        fetched_at: Timestamp(1000),
    };
    let human = store::profiles::AccountInfoCache {
        did: "did:plc:human".into(),
        display_name: String::new(),
        is_bot: false,
        fetched_at: Timestamp(1000),
    };
    store.upsert_account_info(&bot).await.unwrap();
    store.upsert_account_info(&human).await.unwrap();

    // Only the bot DID comes back — this is what the conversation-load path
    // uses to keep an auto-accepted bot DM out of the message-request gate.
    let bots = store.list_bot_dids().await.unwrap();
    assert_eq!(bots, vec!["did:local:adminbot".to_string()]);
}

#[tokio::test]
async fn profile_fetch_state_round_trip() {
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    let did = "did:plc:dormant00000000000001";
    assert!(store.load_fetch_state(did).await.unwrap().is_none());

    // Record an attempt; read it back.
    store.record_fetch_attempt(did, 3, Timestamp(5000)).await.unwrap();
    let (at, outcome) = store.load_fetch_state(did).await.unwrap().unwrap();
    assert_eq!(at, Timestamp(5000));
    assert_eq!(outcome, 3);

    // A later attempt overwrites (one row per DID).
    store.record_fetch_attempt(did, 0, Timestamp(9000)).await.unwrap();
    let (at, outcome) = store.load_fetch_state(did).await.unwrap().unwrap();
    assert_eq!(at, Timestamp(9000));
    assert_eq!(outcome, 0);
}

// ── conversation_settings ─────────────────────────────────────────────────────

#[tokio::test]
async fn conversation_expiry_missing_returns_none() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    let result = store.load_conversation_expiry("did:example:alice").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn conversation_expiry_round_trip() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    store.save_conversation_expiry("did:example:alice", Some(3600)).await.unwrap();
    let loaded = store.load_conversation_expiry("did:example:alice").await.unwrap();
    assert_eq!(loaded, Some(3600));
}

#[tokio::test]
async fn conversation_expiry_zero_treated_as_none() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    // Explicit zero means "no expiry"; load returns None.
    store.save_conversation_expiry("did:example:alice", Some(0)).await.unwrap();
    let loaded = store.load_conversation_expiry("did:example:alice").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn conversation_expiry_none_clears_value() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    store.save_conversation_expiry("did:example:alice", Some(86400)).await.unwrap();
    store.save_conversation_expiry("did:example:alice", None).await.unwrap();
    let loaded = store.load_conversation_expiry("did:example:alice").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn conversation_expiry_overwrite() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    store.save_conversation_expiry("did:example:alice", Some(3600)).await.unwrap();
    store.save_conversation_expiry("did:example:alice", Some(604800)).await.unwrap();
    let loaded = store.load_conversation_expiry("did:example:alice").await.unwrap();
    assert_eq!(loaded, Some(604800));
}

#[tokio::test]
async fn conversation_expiry_independent_per_conversation() {
    let store = DeviceStore::open_in_memory().await.unwrap();
    store.save_conversation_expiry("did:example:alice", Some(3600)).await.unwrap();
    store.save_conversation_expiry("did:example:bob", Some(86400)).await.unwrap();
    assert_eq!(store.load_conversation_expiry("did:example:alice").await.unwrap(), Some(3600));
    assert_eq!(store.load_conversation_expiry("did:example:bob").await.unwrap(), Some(86400));
}

// ── Identity / device wipe (Delete-identity, docs/53) ───────────────────────

#[tokio::test]
async fn wipe_identity_clears_keys_and_durable_state() {
    use store::groups::{GroupRow, PolicyRow};
    use types::Timestamp;

    let store = DeviceStore::open_in_memory().await.unwrap();

    // Seed identity-scoped state: keys + a group master key.
    store
        .save_identity_keypair(&crypto::IdentityKeyPair::generate())
        .await
        .unwrap();
    store.save_did("did:plc:wipeme01", Timestamp(123)).await.unwrap();
    store.save_rotation_key(&[1u8; 32], &[2u8; 33]).await.unwrap();
    store
        .save_group(&GroupRow {
            group_id: "g-wipe".into(),
            master_key: vec![7u8; 32],
            hosting_server_url: "https://hs.example".into(),
            revision: 0,
            encrypted_state_plaintext: Vec::new(),
            policy: PolicyRow::default_admin_only(),
            group_push_pseudonym: None,
            created_at: Timestamp(1),
        })
        .await
        .unwrap();

    // Sanity: present before wipe.
    assert!(store.load_identity().await.unwrap().is_some());
    assert!(store.load_rotation_key().await.unwrap().is_some());
    assert!(store.load_did().await.unwrap().is_some());
    assert_eq!(store.list_groups().await.unwrap().len(), 1);

    store.wipe_identity().await.unwrap();

    // Nothing survives — no key material, no group master keys.
    assert!(store.load_identity().await.unwrap().is_none());
    assert!(store.load_rotation_key().await.unwrap().is_none());
    assert!(store.load_did().await.unwrap().is_none());
    assert!(store.list_groups().await.unwrap().is_empty());
}

#[tokio::test]
async fn wipe_device_clears_registration_and_cursor() {
    use store::account::DeviceAccount;
    use types::Timestamp;

    let store = DeviceStore::open_in_memory().await.unwrap();

    store
        .save_device_account(&DeviceAccount {
            server_url: "https://hs.example".into(),
            device_id: 7,
            registered_at: Timestamp(123),
            registration_id: 4242,
        })
        .await
        .unwrap();
    store.set_storage_cursor(99).await.unwrap();

    assert!(store.load_device_account().await.unwrap().is_some());
    assert_eq!(store.storage_cursor().await.unwrap(), 99);

    store.wipe_device().await.unwrap();

    assert!(store.load_device_account().await.unwrap().is_none());
    assert_eq!(store.storage_cursor().await.unwrap(), 0);
}

// ── Storage-service sidecar & dirty-tracking triggers (docs/05) ──────────────

#[cfg(test)]
mod storage_sync {
    use store::groups::{GroupRow, PolicyRow};
    use store::storage_sync::SyncTriggerSpec;
    use store::DeviceStore;
    use types::Timestamp;

    /// Open a store and install the `groups` dirty-tracking trigger. The triggers
    /// are no longer baked into the schema (docs/05 stage 3); app-core installs
    /// them from its sync registry at account open, so trigger-dependent tests
    /// install the same spec here.
    async fn store_with_group_triggers() -> DeviceStore {
        let store = DeviceStore::open_in_memory().await.unwrap();
        store
            .install_sync_triggers(&[SyncTriggerSpec::new("groups", "group_id", 1)])
            .await
            .unwrap();
        store
    }

    fn sample_group(id: &str) -> GroupRow {
        GroupRow {
            group_id: id.to_string(),
            master_key: vec![7u8; 32],
            hosting_server_url: "https://hs.example".into(),
            revision: 0,
            encrypted_state_plaintext: Vec::new(),
            policy: PolicyRow::default_admin_only(),
            group_push_pseudonym: None,
            created_at: Timestamp::now(),
        }
    }

    #[tokio::test]
    async fn storage_key_round_trips() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        assert!(store.load_storage_key().await.unwrap().is_none());
        let key = [3u8; 32];
        store.save_storage_key(&key).await.unwrap();
        assert_eq!(store.load_storage_key().await.unwrap(), Some(key));
    }

    #[tokio::test]
    async fn cursor_defaults_zero_and_advances() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        assert_eq!(store.storage_cursor().await.unwrap(), 0);
        store.set_storage_cursor(42).await.unwrap();
        assert_eq!(store.storage_cursor().await.unwrap(), 42);
    }

    #[tokio::test]
    async fn group_insert_marks_sidecar_dirty() {
        let store = store_with_group_triggers().await;
        store.save_group(&sample_group("group-abc")).await.unwrap();

        let dirty = store.dirty_records().await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].type_tag, 1); // TYPE_GROUP_KEY
        assert_eq!(dirty[0].logical_key, "group-abc");
        assert!(!dirty[0].deleted);
        assert_eq!(dirty[0].version, 0);
    }

    #[tokio::test]
    async fn group_delete_marks_sidecar_tombstone_keeping_version() {
        let store = store_with_group_triggers().await;
        store.save_group(&sample_group("group-del")).await.unwrap();
        // Mimic a successful push so the row is clean at version 5.
        store.set_sync_meta_clean(1, "group-del", 5).await.unwrap();
        assert!(store.dirty_records().await.unwrap().is_empty());

        store.delete_group("group-del").await.unwrap();
        let dirty = store.dirty_records().await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert!(dirty[0].deleted);
        // Version is preserved so the tombstone push CASes against version 5.
        assert_eq!(dirty[0].version, 5);
    }

    #[tokio::test]
    async fn set_sync_meta_clears_dirty_and_records_version() {
        let store = store_with_group_triggers().await;
        store.save_group(&sample_group("g1")).await.unwrap();
        assert_eq!(store.dirty_records().await.unwrap().len(), 1);

        // Pull-side apply: record the server version and clear dirty.
        store.set_sync_meta(1, "g1", 9, false, false).await.unwrap();
        assert!(store.dirty_records().await.unwrap().is_empty());
        assert_eq!(store.sync_version(1, "g1").await.unwrap(), 9);
    }

    #[tokio::test]
    async fn update_group_state_noop_does_not_redirty() {
        let store = store_with_group_triggers().await;
        let g = sample_group("g-noop");
        store.save_group(&g).await.unwrap();
        // Clear the insert's dirty mark (mimic a successful push).
        store.set_sync_meta(1, "g-noop", 1, false, false).await.unwrap();
        assert!(store.dirty_records().await.unwrap().is_empty());

        // Re-fetch with byte-identical state — the common case when you open a
        // group and the server revision hasn't moved. Must NOT re-dirty: the
        // guarded UPDATE matches zero rows, so the trigger never fires.
        store
            .update_group_state(
                "g-noop",
                g.revision,
                g.encrypted_state_plaintext.clone(),
                PolicyRow::default_admin_only(),
            )
            .await
            .unwrap();
        assert!(
            store.dirty_records().await.unwrap().is_empty(),
            "a no-op refetch must not mark the group dirty"
        );

        // A genuine change (new revision + state) does dirty — a sync then is
        // expected and fine.
        store
            .update_group_state(
                "g-noop",
                g.revision + 1,
                b"new-state".to_vec(),
                PolicyRow::default_admin_only(),
            )
            .await
            .unwrap();
        let dirty = store.dirty_records().await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].logical_key, "g-noop");
    }

    #[tokio::test]
    async fn noop_update_group_state_does_not_poke_commit_hook() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        // A 0-row UPDATE still commits a write transaction and fires SQLite's
        // commit hook — which pokes the storage-sync scheduler into a redundant
        // pull. So `update_group_state` must skip the write entirely (read-only
        // compare) on an unchanged refetch, the common case when opening a group.
        let store = store_with_group_triggers().await;
        let g = sample_group("g-hook");
        store.save_group(&g).await.unwrap();

        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        store
            .set_commit_hook(move || {
                c2.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap();

        // No-op refetch: identical values → no write → no commit-hook poke.
        store
            .update_group_state(
                "g-hook",
                g.revision,
                g.encrypted_state_plaintext.clone(),
                PolicyRow::default_admin_only(),
            )
            .await
            .unwrap();
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "a no-op group-state refetch must not poke the sync scheduler"
        );

        // A genuine change writes and fires the hook (a sync then is fine).
        store
            .update_group_state("g-hook", g.revision + 1, b"x".to_vec(), PolicyRow::default_admin_only())
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sync_version_defaults_zero_for_unknown_record() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        assert_eq!(store.sync_version(1, "never-seen").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn install_sync_triggers_generates_per_table_dirty_tracking() {
        // Prove the generator works for an arbitrary table/key/tag, not just the
        // hardcoded groups triggers it replaced.
        let store = DeviceStore::open_in_memory().await.unwrap();
        store
            .install_sync_triggers(&[SyncTriggerSpec::new("contacts", "did", 2)])
            .await
            .unwrap();
        assert!(store.dirty_records().await.unwrap().is_empty());

        store
            .touch_contact("did:plc:abc", true, Timestamp(123))
            .await
            .unwrap();
        let dirty = store.dirty_records().await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].type_tag, 2);
        assert_eq!(dirty[0].logical_key, "did:plc:abc");
        assert!(!dirty[0].deleted);

        store.delete_contact("did:plc:abc").await.unwrap();
        let dirty = store.dirty_records().await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert!(dirty[0].deleted, "delete marks a tombstone");
    }

    #[tokio::test]
    async fn cursor_write_is_skipped_when_unchanged() {
        // The commit-hook scheduler relies on set_storage_cursor not committing
        // when the value is unchanged, so a settled sync quiesces (docs/05 §6.1).
        let store = store_with_group_triggers().await;
        store.set_storage_cursor(7).await.unwrap();
        // Re-writing the same value must not resurrect a cleared dirty bit via a
        // spurious commit; here we just assert the value is stable + idempotent.
        store.set_storage_cursor(7).await.unwrap();
        assert_eq!(store.storage_cursor().await.unwrap(), 7);
    }
}

// ── Editing, deletion, reactions (docs/33, docs/36) ──────────────────────

#[cfg(test)]
mod edit_delete_react {
    use store::messages::{HistoryMessage, ReactionRow};
    use store::DeviceStore;
    use types::Timestamp;

    async fn seed(store: &DeviceStore, conv: &str, author: &str, sent_at: i64, body: &str) {
        store
            .save_message(&HistoryMessage {
                id: format!("{author}-{sent_at}"),
                conversation_id: conv.into(),
                sender_did: author.into(),
                body: body.into(),
                sent_at: Timestamp(sent_at),
                edited_at: None,
                read_at: None,
                delivery_status: 1,
                edit_count: 0,
                deleted_at: None,
                kind: 0,
                metadata: None,
                expire_timer_secs: 0,
                expire_at: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn edit_updates_body_bumps_count_and_records_revision() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        seed(&store, "dm-me-bob", "did:bob", 100, "helo").await;

        let applied = store
            .apply_edit("dm-me-bob", "did:bob", Timestamp(100), "hello", Timestamp(200), true)
            .await
            .unwrap();
        assert!(applied);

        let m = store.find_message("dm-me-bob", "did:bob", Timestamp(100)).await.unwrap().unwrap();
        assert_eq!(m.body, "hello");
        assert_eq!(m.edit_count, 1);
        assert_eq!(m.edited_at, Some(Timestamp(200)));

        let revs = store.load_revisions("dm-me-bob", "did:bob", Timestamp(100)).await.unwrap();
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].body, "helo");
    }

    #[tokio::test]
    async fn edit_is_last_writer_wins() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        seed(&store, "c", "a", 100, "v1").await;
        store.apply_edit("c", "a", Timestamp(100), "v2", Timestamp(300), true).await.unwrap();
        // An older edit (op time 200 < 300) must be ignored.
        store.apply_edit("c", "a", Timestamp(100), "stale", Timestamp(200), true).await.unwrap();
        let m = store.find_message("c", "a", Timestamp(100)).await.unwrap().unwrap();
        assert_eq!(m.body, "v2");
        assert_eq!(m.edit_count, 1);
    }

    #[tokio::test]
    async fn edit_of_missing_target_is_dropped() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        let applied = store
            .apply_edit("c", "a", Timestamp(100), "x", Timestamp(200), true)
            .await
            .unwrap();
        assert!(!applied);
        assert!(store.find_message("c", "a", Timestamp(100)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn tombstone_clears_body_drops_reactions_and_absorbs_edits() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        seed(&store, "c", "a", 100, "secret").await;
        store
            .upsert_reaction(&ReactionRow {
                conversation_id: "c".into(),
                target_author: "a".into(),
                target_sent_at: Timestamp(100),
                reactor_did: "b".into(),
                emoji: "👍".into(),
                reacted_at: Timestamp(150),
            })
            .await
            .unwrap();

        store.tombstone_message("c", "a", Timestamp(100), Timestamp(400)).await.unwrap();

        let m = store.find_message("c", "a", Timestamp(100)).await.unwrap().unwrap();
        assert_eq!(m.body, "");
        assert_eq!(m.deleted_at, Some(Timestamp(400)));
        assert!(store.load_reactions("c").await.unwrap().is_empty());

        // A late edit can't resurrect a tombstone.
        store.apply_edit("c", "a", Timestamp(100), "back", Timestamp(500), true).await.unwrap();
        let m = store.find_message("c", "a", Timestamp(100)).await.unwrap().unwrap();
        assert_eq!(m.body, "");
        assert!(m.deleted_at.is_some());
    }

    #[tokio::test]
    async fn delete_for_me_removes_row_and_reactions() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        seed(&store, "c", "a", 100, "bye").await;
        store
            .upsert_reaction(&ReactionRow {
                conversation_id: "c".into(),
                target_author: "a".into(),
                target_sent_at: Timestamp(100),
                reactor_did: "b".into(),
                emoji: "❤️".into(),
                reacted_at: Timestamp(150),
            })
            .await
            .unwrap();

        store.delete_message_for_me("c", "a", Timestamp(100)).await.unwrap();
        assert!(store.find_message("c", "a", Timestamp(100)).await.unwrap().is_none());
        assert!(store.load_reactions("c").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reaction_is_one_per_person_and_removable() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        let row = |emoji: &str, at: i64| ReactionRow {
            conversation_id: "c".into(),
            target_author: "a".into(),
            target_sent_at: Timestamp(100),
            reactor_did: "b".into(),
            emoji: emoji.into(),
            reacted_at: Timestamp(at),
        };
        store.upsert_reaction(&row("👍", 150)).await.unwrap();
        store.upsert_reaction(&row("❤️", 160)).await.unwrap();
        // Re-reacting replaces, never duplicates: one row, latest emoji.
        let rs = store.load_reactions("c").await.unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].emoji, "❤️");

        store.remove_reaction("c", "a", Timestamp(100), "b").await.unwrap();
        assert!(store.load_reactions("c").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn distinct_reactors_each_get_a_row() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        for reactor in ["b", "c", "d"] {
            store
                .upsert_reaction(&ReactionRow {
                    conversation_id: "conv".into(),
                    target_author: "a".into(),
                    target_sent_at: Timestamp(100),
                    reactor_did: reactor.into(),
                    emoji: "👍".into(),
                    reacted_at: Timestamp(150),
                })
                .await
                .unwrap();
        }
        assert_eq!(store.load_reactions("conv").await.unwrap().len(), 3);
    }
}

// ── Contacts: blocking + pending request (docs/12 §2, docs/52) ────────────

#[cfg(test)]
mod contacts_block_pending {
    use store::DeviceStore;
    use types::Timestamp;

    #[tokio::test]
    async fn block_creates_bare_row_without_curating() {
        // docs/12 §2: blocking a never-seen DID creates a row with only
        // is_blocked set; is_curated stays false.
        let store = DeviceStore::open_in_memory().await.unwrap();
        store.set_blocked("did:plc:stranger", true).await.unwrap();

        let row = store.load_contact("did:plc:stranger").await.unwrap().unwrap();
        assert!(row.is_blocked);
        assert!(!row.is_curated, "blocking does not curate");

        let blocked = store.list_blocked().await.unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].did, "did:plc:stranger");
    }

    #[tokio::test]
    async fn unblock_clears_the_flag_and_leaves_list() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        store.set_blocked("did:plc:x", true).await.unwrap();
        store.set_blocked("did:plc:x", false).await.unwrap();

        assert!(!store.load_contact("did:plc:x").await.unwrap().unwrap().is_blocked);
        assert!(store.list_blocked().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn block_preserves_existing_curation() {
        // A curated contact stays curated through a block (the relationship is
        // remembered; block only overrides visibility).
        let store = DeviceStore::open_in_memory().await.unwrap();
        store.touch_contact("did:plc:friend", true, Timestamp(100)).await.unwrap();
        store.set_blocked("did:plc:friend", true).await.unwrap();

        let row = store.load_contact("did:plc:friend").await.unwrap().unwrap();
        assert!(row.is_curated);
        assert!(row.is_blocked);
    }

    #[tokio::test]
    async fn pending_request_toggles_independently() {
        let store = DeviceStore::open_in_memory().await.unwrap();
        store.set_pending_request("did:plc:req", true).await.unwrap();
        assert!(store.load_contact("did:plc:req").await.unwrap().unwrap().has_pending_request);

        store.set_pending_request("did:plc:req", false).await.unwrap();
        assert!(!store.load_contact("did:plc:req").await.unwrap().unwrap().has_pending_request);
    }

    #[tokio::test]
    async fn apply_synced_contact_overwrites_block_but_maxes_curation() {
        // Sync LWW (docs/05): the engine applies only strictly-newer versions,
        // so is_blocked is overwritten with the pulled value (so an unblock
        // elsewhere propagates) while is_curated/recency take a monotonic MAX.
        let store = DeviceStore::open_in_memory().await.unwrap();
        // Local state: curated, blocked, recent.
        store.touch_contact("did:plc:p", true, Timestamp(500)).await.unwrap();
        store.set_blocked("did:plc:p", true).await.unwrap();

        // Pull a record that unblocks, is not curated, and is older.
        store
            .apply_synced_contact("did:plc:p", false, false, Timestamp(100))
            .await
            .unwrap();

        let row = store.load_contact("did:plc:p").await.unwrap().unwrap();
        assert!(!row.is_blocked, "block overwritten by pulled value");
        assert!(row.is_curated, "curation never rewinds (MAX)");
        assert_eq!(row.last_interaction_at, Timestamp(500), "recency never rewinds (MAX)");
    }
}

/// docs/03 §5 disappearing-messages: outgoing starts its countdown at send,
/// incoming starts on read, `timer = 0` never expires, and the sweep returns
/// the affected conversations.
#[tokio::test]
async fn disappearing_messages_expire_lifecycle() {
    use store::messages::HistoryMessage;
    use types::Timestamp;
    let store = DeviceStore::open_in_memory().await.unwrap();

    let base = 1_000_000i64; // ms
    let row = |id: &str, sender: &str, read: Option<i64>, timer: i64| HistoryMessage {
        id: id.into(),
        conversation_id: "group-g".into(),
        sender_did: sender.into(),
        body: "hi".into(),
        sent_at: Timestamp(base),
        edited_at: None,
        read_at: read.map(Timestamp),
        delivery_status: 1,
        edit_count: 0,
        deleted_at: None,
        kind: 0,
        metadata: None,
        expire_timer_secs: timer,
        expire_at: None,
    };

    // Outgoing (read at send) with a 30s timer → deadline = base + 30s.
    store.save_message(&row("out", "me", Some(base), 30)).await.unwrap();
    // Incoming (unread) with a 30s timer → no deadline until read.
    store.save_message(&row("in", "bob", None, 30)).await.unwrap();
    // No timer → never expires.
    store.save_message(&row("perm", "bob", Some(base), 0)).await.unwrap();

    // Nothing due yet.
    assert!(store
        .delete_expired_messages(Timestamp(base + 1_000))
        .await
        .unwrap()
        .is_empty());

    // Past the outgoing deadline: only "out" is swept.
    let gone = store.delete_expired_messages(Timestamp(base + 31_000)).await.unwrap();
    assert_eq!(gone, vec!["group-g".to_string()]);
    let ids: Vec<String> = store
        .load_messages("group-g", Timestamp(base + 31_000))
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert!(!ids.contains(&"out".into()), "outgoing expired");
    assert!(ids.contains(&"in".into()), "unread incoming has no deadline yet");
    assert!(ids.contains(&"perm".into()), "no-timer message never expires");

    // Reading the incoming message starts its countdown (deadline = read + 30s).
    store
        .mark_messages_read("group-g", Timestamp(base + 31_000), Timestamp(base + 31_000))
        .await
        .unwrap();
    assert!(store
        .delete_expired_messages(Timestamp(base + 40_000))
        .await
        .unwrap()
        .is_empty(), "incoming not due yet");
    store.delete_expired_messages(Timestamp(base + 62_000)).await.unwrap();
    let ids: Vec<String> = store
        .load_messages("group-g", Timestamp(base + 62_000))
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert!(!ids.contains(&"in".into()), "read incoming expired");
    assert!(ids.contains(&"perm".into()), "no-timer message still present");
}
