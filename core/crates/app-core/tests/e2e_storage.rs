//! End-to-end integration test for the storage service / device-data-sync
//! engine (docs/05-device-data-sync.md, stage 2).
//!
//! Exercises the full client engine against a live homeserver:
//!
//! 1. An account creates a group → the `groups` row is written and the
//!    `storage_sync` sidecar is marked dirty by the trigger.
//! 2. `sync_storage` PUSHes the group-key record (encrypted) to the server.
//! 3. We simulate a fresh device that shares the identity's storage key but
//!    has never seen this record: drop the local `groups` row, neutralize the
//!    sidecar, and rewind the cursor.
//! 4. `sync_storage` PULLs the record back, decrypts it, routes it by TYPE_TAG,
//!    and writes the master key back through into the `groups` table.
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

mod common;

use app_core::storage_sync::{TYPE_CONTACT, TYPE_CONTACT_PROFILE, TYPE_CONV_SETTINGS};
use app_core::AppCore;
use store::profiles::ContactProfile;
use types::Timestamp;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

/// Enable storage sync on a store the way a human account's creation does:
/// provision a storage key and install the dirty-tracking triggers.
///
/// The engine and adapters are account-type-agnostic — the bot opt-out only
/// skips *provisioning the key* (docs/05 §11). These tests use bot accounts (as
/// every e2e test here does, since a human account needs a minted PLC DID the
/// harness avoids) and inject the key directly, exercising the full push/pull
/// machinery without that orthogonal concern. The opt-out itself is unit-tested.
async fn enable_storage_sync(store: &store::DeviceStore) {
    store.save_storage_key(&[42u8; 32]).await.unwrap();
    app_core::storage_sync::ensure_triggers(store).await.unwrap();
}

#[tokio::test]
async fn storage_push_then_pull_restores_group_key() {
    let url = server_url();
    // Keep a handle on the store so we can simulate device loss directly.
    let store = test_store().await;
    let alice = AppCore::create_account_with_store(&url, store.clone(), None, true, common::invite_token())
        .await
        .unwrap();
    enable_storage_sync(&store).await;

    // 1. Create a group → groups row written + sidecar marked dirty by trigger.
    let created = alice.create_group_async("Sync", "storage e2e", 0).await.unwrap();
    assert_eq!(created.master_key.len(), 32);
    let original = store
        .load_group(&created.group_id)
        .await
        .unwrap()
        .expect("group row exists after create");

    // 2. Push the group-key record to the authoritative server.
    alice.sync_storage_async().await.unwrap();

    // 3. Simulate a fresh device of the same identity (same storage key, same
    //    account) that has never pulled this record: drop the local row,
    //    neutralize its sidecar so the next sync won't push a tombstone, and
    //    rewind both the per-record version and the pull cursor to 0.
    store.delete_group(&created.group_id).await.unwrap();
    store
        .set_sync_meta(1 /* TYPE_GROUP_KEY */, &created.group_id, 0, false, false)
        .await
        .unwrap();
    store.set_storage_cursor(0).await.unwrap();
    assert!(
        store.load_group(&created.group_id).await.unwrap().is_none(),
        "group row removed to simulate device loss"
    );

    // 4. Pull restores the record: decrypt → route by tag → write-through.
    alice.sync_storage_async().await.unwrap();

    let restored = store
        .load_group(&created.group_id)
        .await
        .unwrap()
        .expect("group row restored by storage pull");
    assert_eq!(restored.master_key, original.master_key);
    assert_eq!(restored.hosting_server_url, original.hosting_server_url);
}

/// Stage-3 adapters: a contact (curation), its profile (name + profile_key),
/// and a conversation-expiry setting all roam across devices. Exercises the
/// `SyncedType` blanket bridge, trigger generation for the three new tables,
/// and pull-side write-through, the same push → simulated-fresh-device → pull
/// shape as the group-key test above.
#[tokio::test]
async fn contact_profile_and_settings_round_trip() {
    let url = server_url();
    let store = test_store().await;
    let alice = AppCore::create_account_with_store(&url, store.clone(), None, true, common::invite_token())
        .await
        .unwrap();
    enable_storage_sync(&store).await;

    let did = "did:plc:contact-roams";
    let cid = "did:plc:some-conversation";

    // 1. Local mutations → triggers mark the sidecar dirty (no per-write code).
    store.touch_contact(did, true, Timestamp(1_700_000_000_000)).await.unwrap();
    store
        .upsert_contact_profile(&ContactProfile {
            did: did.to_string(),
            display_name: "Roaming Rita".into(),
            profile_key: vec![7u8; 32],
            fetched_at: Timestamp(1_700_000_111_111),
        })
        .await
        .unwrap();
    store.save_conversation_expiry(cid, Some(3600)).await.unwrap();

    // 2. Push to the authoritative server.
    alice.sync_storage_async().await.unwrap();

    // 3. Simulate a fresh device of the same identity: drop the local rows,
    //    neutralize the tombstones the delete triggers just set, and rewind both
    //    the per-record versions and the pull cursor to 0.
    store.delete_contact(did).await.unwrap();
    store.delete_contact_profile(did).await.unwrap();
    store.delete_conversation_settings(cid).await.unwrap();
    store.set_sync_meta(TYPE_CONTACT, did, 0, false, false).await.unwrap();
    store.set_sync_meta(TYPE_CONTACT_PROFILE, did, 0, false, false).await.unwrap();
    store.set_sync_meta(TYPE_CONV_SETTINGS, cid, 0, false, false).await.unwrap();
    store.set_storage_cursor(0).await.unwrap();
    assert!(store.load_contact(did).await.unwrap().is_none());

    // 4. Pull restores all three, routed by TYPE_TAG and written through.
    alice.sync_storage_async().await.unwrap();

    let contact = store
        .load_contact(did)
        .await
        .unwrap()
        .expect("contact row restored by storage pull");
    assert!(contact.is_curated);
    assert_eq!(contact.last_interaction_at.as_millis(), 1_700_000_000_000);

    let profile = store
        .load_contact_profile(did)
        .await
        .unwrap()
        .expect("contact profile restored by storage pull");
    assert_eq!(profile.display_name, "Roaming Rita");
    assert_eq!(profile.profile_key, vec![7u8; 32]);

    assert_eq!(
        store.load_conversation_expiry(cid).await.unwrap(),
        Some(3600),
        "conversation timer restored by storage pull"
    );
}
