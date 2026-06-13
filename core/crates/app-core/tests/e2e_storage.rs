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
async fn storage_push_then_pull_restores_group_key() {
    let url = server_url();
    // Keep a handle on the store so we can simulate device loss directly.
    let store = test_store().await;
    let alice = AppCore::create_account_with_store(&url, store.clone(), None, true)
        .await
        .unwrap();

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
