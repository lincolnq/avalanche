//! Shared test helpers for the actnet workspace.
//!
//! Provides factory functions for creating test identities, prekey bundles,
//! and paired client sessions so that integration tests across `crypto` and
//! `store` can be written concisely without repeating setup boilerplate.

use crypto::{self, IdentityKeyPair};
use store::account::DeviceAccount;
use store::DeviceStore;
use types::{AccountId, DeviceId, Timestamp};

/// A test client: an identity, a local device store, and addressing info.
pub struct TestClient {
    pub identity: IdentityKeyPair,
    pub registration_id: u32,
    pub store: DeviceStore,
    pub address: crypto::DeviceAddress,
}

impl TestClient {
    /// Create a new test client with an in-memory store, generate identity and
    /// prekeys, and persist them to the store.
    pub async fn new(name: &str, device_id: u32) -> Self {
        let identity = IdentityKeyPair::generate();
        let registration_id = rand::random::<u32>() & 0x3FFF; // 14-bit, per Signal spec
        let store = DeviceStore::open_in_memory().await.expect("open in-memory store");

        // keypair → identity.db; registration_id → device.db (device_account).
        store
            .save_identity_keypair(&identity)
            .await
            .expect("save identity keypair");
        store
            .save_device_account(&DeviceAccount {
                server_url: String::new(),
                device_id,
                registered_at: Timestamp(0),
                registration_id,
            })
            .await
            .expect("save device account");

        let address = crypto::DeviceAddress::new(
            AccountId::new(name),
            DeviceId::new(device_id),
        );

        Self {
            identity,
            registration_id,
            store,
            address,
        }
    }

    /// Generate a signed prekey + one-time prekeys + Kyber prekey, persist
    /// them to the store, and return a `RecipientKeyBundle` that another
    /// client can use to initiate a session.
    pub async fn publish_prekeys(&self) -> crypto::RecipientKeyBundle {
        let signed = crypto::prekeys::generate_signed_prekey(&self.identity, 1)
            .expect("gen signed prekey");
        self.store
            .save_signed_prekey(signed.wire.id, &signed.record)
            .await
            .expect("save signed prekey");

        let one_time = crypto::prekeys::generate_one_time_prekeys(1, 10)
            .expect("gen one-time prekeys");
        let records: Vec<(u32, Vec<u8>)> = one_time
            .iter()
            .map(|k| (k.wire.id, k.record.clone()))
            .collect();
        self.store
            .save_one_time_prekeys(&records)
            .await
            .expect("save one-time prekeys");

        let kyber = crypto::prekeys::generate_kyber_prekey(&self.identity, 1)
            .expect("gen kyber prekey");
        self.store
            .save_kyber_prekeys(&[(kyber.wire.id, kyber.record.clone())])
            .await
            .expect("save kyber prekey");

        crypto::RecipientKeyBundle {
            identity_key: self.identity.public_key().serialize(),
            registration_id: self.registration_id,
            device_id: self.address.device_id.0,
            signed_prekey: signed.wire,
            one_time_prekey: Some(one_time[0].wire.clone()),
            kyber_prekey: kyber.wire,
        }
    }
}
