//! End-to-end integration test for device linking (docs/04-multi-device.md §4).
//!
//! Exercises the full provisioning channel against a live homeserver: the
//! ephemeral mailbox endpoints, the two-sided ECDH handshake, and the sealed
//! `ProvisioningBundle` round-trip — in **both** directions (existing device
//! shows the code; new device shows the code), proving role is independent of
//! who shows vs. scans.
//!
//! The existing-device side uses the real FFI methods
//! (`link_create_pairing` / `link_accept_pairing` / `link_send_bundle`). The
//! new-device side is driven with the public building blocks
//! (`PairingCode`, `EphemeralKeyPair`, `open_bundle`).
//!
//! **What this does NOT cover:** the final registration step
//! (`provision_linked_device`, reached via `DeviceLinkNew::await_link`). It
//! requires a `did:plc:` identity whose rotation key is resolvable in the live
//! PLC directory (`https://plc.directory`) — the same constraint that leaves
//! `recover_from_blob` untested e2e (docs/05 §13.2). The bundle round-trip
//! asserted here is the load-bearing, PLC-independent part; the registration is
//! covered by the server `link_device` validation tests and the design.
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

mod common;

use app_core::provisioning::{
    derive_shared_key, open_bundle, PairingCode, ProvisioningBundle, SLOT_BUNDLE, SLOT_HANDSHAKE,
};
use app_core::{AppCore, DeviceLinkNew};

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

/// Create an existing (already-provisioned) device with a known storage key so
/// the test can assert the key is transported in the bundle. Uses a bot account
/// (every e2e test here does — a human account needs a minted PLC DID the
/// harness avoids); the bundle/handshake machinery is identity-type-agnostic.
async fn existing_device_with_storage_key(storage_key: [u8; 32]) -> AppCore {
    let url = server_url();
    let store = test_store().await;
    let core = AppCore::create_account_with_store(&url, store.clone(), None, true, common::invite_token())
        .await
        .unwrap();
    // Bots opt out of storage-key provisioning; inject one so the bundle carries
    // it (the store is Arc-backed, so this clone shares the account's connection).
    store.save_storage_key(&storage_key).await.unwrap();
    core
}

#[tokio::test]
async fn device_link_round_trip_existing_shows_code() {
    let url = server_url();
    let storage_key = [7u8; 32];
    let existing = existing_device_with_storage_key(storage_key).await;

    // 1. Existing device shows a pairing code (creates the mailbox session).
    let code = existing
        .link_create_pairing_async(Some(url.clone()))
        .await
        .unwrap();
    let pc = PairingCode::decode(&code).unwrap();

    // 2. New device ingests the code, generates its ephemeral key, and posts its
    //    public half to the handshake slot.
    let new_eph = crypto::EphemeralKeyPair::generate();
    let mailbox = net::Client::new(&pc.mailbox_url);
    mailbox
        .put_provisioning_slot(&pc.session_id, SLOT_HANDSHAKE, &new_eph.public_bytes())
        .await
        .unwrap();

    // 3. Existing device derives K (peer key from the handshake slot it polls),
    //    seals the bundle, and posts it.
    existing.link_send_bundle_async().await.unwrap();

    // 4. New device derives the same K (peer key from the code it scanned) and
    //    opens the bundle.
    let key = derive_shared_key(&new_eph.agree(&pc.ephemeral_pub).unwrap());
    let sealed = mailbox
        .get_provisioning_slot(&pc.session_id, SLOT_BUNDLE)
        .await
        .unwrap()
        .expect("bundle slot present after link_send_bundle");
    let bundle = open_bundle(&sealed, &key).unwrap();

    assert_bundle_matches(&bundle, &existing.did_async().await, &storage_key);
}

#[tokio::test]
async fn device_link_round_trip_new_shows_code() {
    let url = server_url();
    let storage_key = [9u8; 32];
    let existing = existing_device_with_storage_key(storage_key).await;

    // 1. New device shows a pairing code (creates the mailbox session itself).
    let mailbox = net::Client::new(&url);
    let session = mailbox.create_provisioning_session().await.unwrap();
    let new_eph = crypto::EphemeralKeyPair::generate();
    let code = PairingCode {
        mailbox_url: url.clone(),
        session_id: session.session_id.clone(),
        ephemeral_pub: new_eph.public_bytes(),
    }
    .encode();

    // 2. Existing device scans the code (posts its own ephemeral key to the
    //    handshake slot) and seals + posts the bundle.
    existing.link_accept_pairing_async(code).await.unwrap();
    existing.link_send_bundle_async().await.unwrap();

    // 3. New device reads the existing device's key from the handshake slot,
    //    derives K, and opens the bundle.
    let existing_pub = mailbox
        .get_provisioning_slot(&session.session_id, SLOT_HANDSHAKE)
        .await
        .unwrap()
        .expect("existing device posted its ephemeral key on accept");
    let key = derive_shared_key(&new_eph.agree(&existing_pub).unwrap());
    let sealed = mailbox
        .get_provisioning_slot(&session.session_id, SLOT_BUNDLE)
        .await
        .unwrap()
        .expect("bundle slot present after link_send_bundle");
    let bundle = open_bundle(&sealed, &key).unwrap();

    assert_bundle_matches(&bundle, &existing.did_async().await, &storage_key);
}

#[tokio::test]
async fn device_link_new_handle_creates_session_and_posts_handshake() {
    // Exercises the new-device FFI object up to the PLC boundary: `create_pairing`
    // yields a decodable code backed by a real mailbox session, and a peer's
    // `accept_pairing` posts its ephemeral key to that session's handshake slot.
    let url = server_url();

    let shower = DeviceLinkNew::new();
    let code = shower.create_pairing_async(Some(url.clone())).await.unwrap();
    let pc = PairingCode::decode(&code).unwrap();
    assert_eq!(pc.mailbox_url, url);
    assert!(!pc.session_id.is_empty());
    assert_eq!(pc.ephemeral_pub.len(), 33, "serialized Curve25519 public key");

    let scanner = DeviceLinkNew::new();
    scanner.accept_pairing_async(code).await.unwrap();

    let mailbox = net::Client::new(&url);
    let posted = mailbox
        .get_provisioning_slot(&pc.session_id, SLOT_HANDSHAKE)
        .await
        .unwrap();
    assert!(posted.is_some(), "scanner posted its ephemeral key to handshake");
    assert_ne!(
        posted.unwrap(),
        pc.ephemeral_pub,
        "scanner posts its OWN key, not the shower's"
    );
}

/// The opened bundle must carry the existing device's identity (the shared
/// credential), its DID, and the injected storage key.
fn assert_bundle_matches(bundle: &ProvisioningBundle, expected_did: &str, storage_key: &[u8; 32]) {
    assert_eq!(bundle.did, expected_did, "bundle carries the identity's DID");
    assert!(!bundle.identity_keypair.is_empty(), "identity keypair transported");
    // The transported identity keypair must deserialize to a usable key.
    crypto::IdentityKeyPair::deserialize(&bundle.identity_keypair)
        .expect("transported identity keypair is valid");
    assert!(!bundle.rotation_key_private.is_empty(), "rotation key transported");
    assert_eq!(bundle.storage_key, storage_key, "storage key transported");
    // The existing (authed) device resolves the new device's registration
    // prerequisites and ships them in the bundle (docs/04 §4.2), since the
    // joining device can't call authed endpoints itself. The existing device is
    // device 1, so the next free id is 2.
    assert!(
        bundle.new_device_id >= 2,
        "bundle carries a free new_device_id past the existing device (got {})",
        bundle.new_device_id
    );
    assert!(
        !bundle.link_nonce.is_empty(),
        "bundle carries an anti-replay link nonce challenged by the existing device"
    );
}
