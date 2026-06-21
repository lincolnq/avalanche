//! End-to-end tests for the account-lifecycle teardown flows (docs/53):
//! self-leaving a group and leaving a server.
//!
//! `delete_identity` is intentionally NOT covered here: it requires a human
//! `did:plc` identity and submits a tombstone to the real PLC directory
//! (the URL is fixed at `https://plc.directory`), which can't be exercised
//! against the local dev server. Its leave-cascade + account-deletion steps are
//! covered transitively by `leave_server` below, and its PLC tombstone
//! build/sign path by the unit tests in `app-core/src/plc.rs`.
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

mod common;

use app_core::AppCore;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

/// Set up `alice` (founder + admin) and `bob` (plain member) in a fresh group.
/// Returns `(alice, bob, group_id)`.
async fn group_with_two_members() -> (AppCore, AppCore, String) {
    let url = server_url();
    let alice = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob = AppCore::create_account_with_store(&url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let bob_did = bob.did_async().await;

    let created = alice.create_group_async("Lifecycle", "leave e2e", 0).await.unwrap();
    alice.invite_member_async(&created.group_id, &bob_did, 0).await.unwrap();
    // Bob receives the GroupContext DM, fetches state (caches it locally), then
    // accepts into membership.
    let _ = bob.receive_messages_async().await.unwrap();
    bob.fetch_group_state_async(&created.group_id).await.unwrap();
    bob.accept_invite_async(&created.group_id).await.unwrap();

    // Alice confirms both are members before the teardown step.
    let state = alice.fetch_group_state_async(&created.group_id).await.unwrap();
    assert_eq!(state.members.len(), 2, "alice + bob are members");

    (alice, bob, created.group_id)
}

#[tokio::test]
async fn leave_group_removes_own_membership() {
    let (alice, bob, group_id) = group_with_two_members().await;

    // Bob leaves the group himself (self-class action — bob is a plain member,
    // not an admin, so this exercises the new `leave` action's eligibility).
    bob.leave_group_async(&group_id).await.unwrap();

    // Alice sees bob gone from the membership.
    let state = alice.fetch_group_state_async(&group_id).await.unwrap();
    assert_eq!(state.members.len(), 1, "bob removed himself");

    // Bob keeps the group locally (tombstone-in-place, docs/53) but is no longer
    // a member — the composer gate reads this.
    assert!(
        !bob.is_group_member_async(&group_id).await.unwrap(),
        "bob is no longer a member of the cached group"
    );

    // A server fetch is now membership-gated → 404 for the ex-member.
    assert!(
        bob.fetch_group_state_async(&group_id).await.is_err(),
        "server gates state behind membership"
    );
}

#[tokio::test]
async fn leave_server_runs_cascade_and_deletes_account() {
    let (alice, bob, group_id) = group_with_two_members().await;

    // Bob leaves the whole server: leave-cascade his groups, then delete his
    // account. A successful return implies `DELETE /v1/accounts` returned 204.
    bob.leave_server_async().await.unwrap();

    // The leave cascade removed bob from the group.
    let state = alice.fetch_group_state_async(&group_id).await.unwrap();
    assert_eq!(state.members.len(), 1, "leave cascade removed bob from the group");
}
