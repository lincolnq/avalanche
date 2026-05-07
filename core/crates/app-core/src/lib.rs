//! Application core — orchestrates crypto, store, and net.
//!
//! This is the main entry point for client applications. It wires together the
//! three foundation crates:
//!
//! - **crypto** — X3DH session initiation, Double Ratchet encrypt/decrypt
//! - **store** — local SQLCipher database for sessions, prekeys, messages
//! - **net** — HTTP client for the homeserver API
//!
//! `app-core` owns the high-level flows: account creation, sending a DM,
//! receiving messages, and prekey management. Mobile apps (via UniFFI) and
//! integration tests call into this crate.
//!
//! # FFI design
//!
//! UniFFI-exported methods are synchronous. They block on an internal tokio
//! runtime. Mobile callers should invoke them from a background
//! thread/dispatch queue, never from the main/UI thread.

pub mod error;

use std::path::Path;
use std::sync::Arc;

use crypto::session::{self, DeviceAddress, EncryptedMessage, MessageKind};
use error::{AppError, AppErrorFfi};
use net::types::{OutboundMessage, RegisterRequest};
use store::account::RegistrationInfo;
use tokio::sync::Mutex;
use types::{AccountId, DeviceId, Timestamp};

uniffi::setup_scaffolding!();

/// Shared tokio runtime for FFI blocking calls. Created once, lives forever.
fn ffi_runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("failed to create tokio runtime"))
}

/// A Project available on the homeserver.
#[derive(uniffi::Record)]
pub struct ProjectInfoFfi {
    pub name: String,
    pub url: String,
    pub description: String,
}

/// A decrypted inbound message.
#[derive(uniffi::Record)]
pub struct DecryptedMessage {
    pub server_id: i64,
    pub sender_did: String,
    pub sender_device_id: u32,
    pub plaintext: Vec<u8>,
}

/// The main client handle. Holds local state and a server connection.
///
/// Thread-safe: all mutable state is behind an async Mutex, and the object
/// is wrapped in Arc by UniFFI. Safe to call from multiple Swift/Kotlin threads.
///
/// Exported methods are blocking — call from a background thread.
#[derive(uniffi::Object)]
pub struct AppCore {
    inner: Mutex<AppCoreInner>,
    /// WebSocket connection for real-time message delivery. Separate from
    /// `inner` so that `send_dm` can proceed while waiting for WS messages.
    ws: Mutex<Option<net::ws::WsConnection>>,
}

struct AppCoreInner {
    store: store::Store,
    client: net::Client,
    local_address: DeviceAddress,
    did: String,
    device_id: u32,
}

#[uniffi::export]
impl AppCore {
    /// Create a new account on the server.
    ///
    /// Call from a background thread — this blocks until complete.
    #[uniffi::constructor]
    pub fn create_account(
        server_url: String,
        db_path: String,
        db_key: String,
    ) -> Result<Arc<Self>, AppErrorFfi> {
        let rt = ffi_runtime();

        let store = rt.block_on(store::Store::open(
            Path::new(&db_path),
            &store::DatabaseKey::from_passphrase(db_key),
        )).map_err(AppError::from).map_err(AppErrorFfi::from)?;

        let inner = rt.block_on(Self::create_inner(&server_url, store))
            .map_err(AppErrorFfi::from)?;

        Ok(Arc::new(Self { inner: Mutex::new(inner), ws: Mutex::new(None) }))
    }

    /// Load an existing account from the local store and authenticate.
    ///
    /// Call from a background thread — this blocks until complete.
    #[uniffi::constructor]
    pub fn login(
        db_path: String,
        db_key: String,
    ) -> Result<Arc<Self>, AppErrorFfi> {
        let rt = ffi_runtime();

        let store = rt.block_on(store::Store::open(
            Path::new(&db_path),
            &store::DatabaseKey::from_passphrase(db_key),
        )).map_err(AppError::from).map_err(AppErrorFfi::from)?;

        let inner = rt.block_on(Self::login_inner(store))
            .map_err(AppErrorFfi::from)?;

        Ok(Arc::new(Self { inner: Mutex::new(inner), ws: Mutex::new(None) }))
    }

    pub fn did(&self) -> String {
        self.inner.blocking_lock().did.clone()
    }

    pub fn device_id(&self) -> u32 {
        self.inner.blocking_lock().device_id
    }

    /// Send an encrypted DM to a recipient.
    ///
    /// Call from a background thread — this blocks until complete.
    pub fn send_dm(
        &self,
        recipient_did: String,
        recipient_device_id: u32,
        plaintext: Vec<u8>,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let mut inner = self.inner.lock().await;
            inner.send_dm(&recipient_did, recipient_device_id, &plaintext).await
        }).map_err(AppErrorFfi::from)
    }

    /// Fetch and decrypt all pending messages from the server.
    ///
    /// Call from a background thread — this blocks until complete.
    pub fn receive_messages(&self) -> Result<Vec<DecryptedMessage>, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let mut inner = self.inner.lock().await;
            inner.receive_messages().await
        }).map_err(AppErrorFfi::from)
    }

    /// Fetch the list of Projects installed on the homeserver.
    pub fn fetch_projects(&self) -> Result<Vec<ProjectInfoFfi>, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let projects = inner.client.fetch_projects().await
                .map_err(AppError::from)?;
            Ok::<_, AppError>(projects.into_iter().map(|p| ProjectInfoFfi {
                name: p.name,
                url: p.url,
                description: p.description,
            }).collect())
        }).map_err(AppErrorFfi::from)
    }

    /// Request a Project token for opening a Project webview.
    pub fn request_project_token(&self, project_url: String) -> Result<String, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let resp = inner.client.request_project_token(&project_url).await
                .map_err(AppError::from)?;
            Ok::<_, AppError>(resp.token)
        }).map_err(AppErrorFfi::from)
    }

    /// Wait for the next message(s) via WebSocket, decrypt, and return.
    ///
    /// Lazily connects on first call. Blocks until at least one message
    /// arrives. Returns an empty vec if the connection is closed (caller
    /// should retry to reconnect).
    ///
    /// Call from a background thread — this blocks until messages arrive.
    pub fn receive_messages_ws(&self) -> Result<Vec<DecryptedMessage>, AppErrorFfi> {
        ffi_runtime().block_on(self.receive_messages_ws_async())
            .map_err(AppErrorFfi::from)
    }
}

// ── Internal async implementation (not exported via FFI) ────────────────────

impl AppCore {
    async fn create_inner(
        server_url: &str,
        store: store::Store,
    ) -> Result<AppCoreInner, AppError> {
        let identity = crypto::IdentityKeyPair::generate();
        let registration_id = rand::Rng::random::<u32>(&mut rand::rng()) & 0x3FFF;
        let device_id = 1u32;

        let signed = crypto::prekeys::generate_signed_prekey(&identity, 1)?;
        let one_time = crypto::prekeys::generate_one_time_prekeys(1, 20)?;
        let kyber = crypto::prekeys::generate_kyber_prekey(&identity, 1)?;

        let client = net::Client::new(server_url);
        let reg_resp = client.register(&RegisterRequest {
            identity_key: identity.public_key().serialize(),
            registration_id: registration_id as i32,
            device_id: device_id as i32,
            signed_prekey_id: signed.wire.id as i32,
            signed_prekey_public: signed.wire.public_key.clone(),
            signed_prekey_signature: signed.wire.signature.clone(),
            one_time_prekeys: one_time.iter().map(|k| (k.wire.id as i32, k.wire.public_key.clone())).collect(),
            kyber_prekey_id: kyber.wire.id as i32,
            kyber_prekey_public: kyber.wire.public_key.clone(),
            kyber_prekey_signature: kyber.wire.signature.clone(),
        }).await?;

        store.save_identity(&identity, registration_id).await?;
        store.save_registration(&RegistrationInfo {
            account_id: reg_resp.did.clone(),
            server_url: server_url.to_string(),
            registered_at: Timestamp::now(),
        }).await?;

        store.save_signed_prekey(signed.wire.id, &signed.record).await?;
        store.save_one_time_prekeys(
            &one_time.iter().map(|k| (k.wire.id, k.record.clone())).collect::<Vec<_>>(),
        ).await?;
        store.save_kyber_prekeys(
            &[(kyber.wire.id, kyber.record.clone())],
        ).await?;

        let client = net::Client::with_token(server_url, reg_resp.session_token);
        let local_address = DeviceAddress::new(
            AccountId::new(&reg_resp.did),
            DeviceId::new(device_id),
        );

        Ok(AppCoreInner {
            store,
            client,
            local_address,
            did: reg_resp.did,
            device_id,
        })
    }

    async fn login_inner(store: store::Store) -> Result<AppCoreInner, AppError> {
        let _identity = store.load_identity().await?
            .ok_or(AppError::NoAccount)?;
        let reg = store.load_registration().await?
            .ok_or(AppError::NoAccount)?;

        let client = net::Client::new(&reg.server_url);
        let auth = client.authenticate(&reg.account_id, 1).await?;
        let client = net::Client::with_token(&reg.server_url, auth.session_token);

        let local_address = DeviceAddress::new(
            AccountId::new(&reg.account_id),
            DeviceId::new(1),
        );

        Ok(AppCoreInner {
            store,
            client,
            local_address,
            did: reg.account_id,
            device_id: 1,
        })
    }

    /// Create account with a pre-opened store (for tests that run inside
    /// an existing tokio runtime).
    pub async fn create_account_with_store(
        server_url: &str,
        store: store::Store,
    ) -> Result<Self, AppError> {
        let inner = Self::create_inner(server_url, store).await?;
        Ok(Self { inner: Mutex::new(inner), ws: Mutex::new(None) })
    }

    /// Login with a pre-opened store (for tests).
    pub async fn login_with_store(store: store::Store) -> Result<Self, AppError> {
        let inner = Self::login_inner(store).await?;
        Ok(Self { inner: Mutex::new(inner), ws: Mutex::new(None) })
    }

    /// Async send_dm for use in tests running inside a tokio runtime.
    pub async fn send_dm_async(
        &self,
        recipient_did: &str,
        recipient_device_id: u32,
        plaintext: &[u8],
    ) -> Result<(), AppError> {
        let mut inner = self.inner.lock().await;
        inner.send_dm(recipient_did, recipient_device_id, plaintext).await
    }

    /// Async receive_messages for use in tests running inside a tokio runtime.
    pub async fn receive_messages_async(&self) -> Result<Vec<DecryptedMessage>, AppError> {
        let mut inner = self.inner.lock().await;
        inner.receive_messages().await
    }

    /// Get DID (async-friendly, for tests).
    pub async fn did_async(&self) -> String {
        self.inner.lock().await.did.clone()
    }

    /// Get device_id (async-friendly, for tests).
    pub async fn device_id_async(&self) -> u32 {
        self.inner.lock().await.device_id
    }

    /// Async version of receive_messages_ws for use in tests and the testbot.
    pub async fn receive_messages_ws_async(&self) -> Result<Vec<DecryptedMessage>, AppError> {
        // 1. Ensure WS is connected.
        {
            let mut ws_guard = self.ws.lock().await;
            if ws_guard.is_none() {
                let inner = self.inner.lock().await;
                let token = inner.client.token()
                    .ok_or_else(|| AppError::Protocol("no session token for WS".into()))?
                    .to_string();
                let url = inner.client.server_url().to_string();
                drop(inner);
                *ws_guard = Some(net::ws::WsConnection::connect(&url, &token).await?);
            }
        }

        // 2. Wait for next message (ws lock only, inner is free for send_dm).
        let raw_msg = {
            let mut ws_guard = self.ws.lock().await;
            let ws_conn = ws_guard.as_mut().unwrap();
            match ws_conn.next_message().await {
                Ok(Some(msg)) => msg,
                Ok(None) => {
                    // Connection closed — clear so next call reconnects.
                    *ws_guard = None;
                    return Ok(vec![]);
                }
                Err(e) => {
                    *ws_guard = None;
                    return Err(e.into());
                }
            }
        };

        // 3. Decrypt (inner lock). If decryption fails, ack the message
        //    anyway so it doesn't block the queue forever, then skip it.
        let decrypted = {
            let mut inner = self.inner.lock().await;
            match inner.decrypt_inbound(&raw_msg).await {
                Ok(msg) => msg,
                Err(e) => {
                    eprintln!(
                        "[ws] failed to decrypt message {} from {:?}: {}, acking to skip",
                        raw_msg.id, raw_msg.sender_did, e
                    );
                    // Ack the undecryptable message so it doesn't block the queue.
                    let mut ws_guard = self.ws.lock().await;
                    if let Some(ws_conn) = ws_guard.as_mut() {
                        let _ = ws_conn.ack(&[raw_msg.id]).await;
                    }
                    // Return empty — caller will loop and get the next message.
                    return Ok(vec![]);
                }
            }
        };

        // 4. Ack via WS.
        {
            let mut ws_guard = self.ws.lock().await;
            if let Some(ws_conn) = ws_guard.as_mut() {
                let _ = ws_conn.ack(&[raw_msg.id]).await;
            }
        }

        Ok(vec![decrypted])
    }
}

impl AppCoreInner {
    async fn send_dm(
        &mut self,
        recipient_did: &str,
        recipient_device_id: u32,
        plaintext: &[u8],
    ) -> Result<(), AppError> {
        let recipient_addr = DeviceAddress::new(
            AccountId::new(recipient_did),
            DeviceId::new(recipient_device_id),
        );

        let has_session = {
            use libsignal_protocol::SessionStore;
            let protocol_addr = libsignal_protocol::ProtocolAddress::new(
                recipient_did.to_string(),
                libsignal_protocol::DeviceId::try_from(recipient_device_id).unwrap(),
            );
            self.store.load_session(&protocol_addr).await
                .map_err(|e| AppError::Crypto(crypto::CryptoError::Signal(e)))?
                .is_some()
        };

        if !has_session {
            let bundle = self.client
                .fetch_prekey_bundle(recipient_did, recipient_device_id as i32)
                .await?;

            let recipient_bundle = crypto::RecipientKeyBundle {
                identity_key: bundle.identity_key.clone(),
                registration_id: bundle.registration_id as u32,
                device_id: recipient_device_id,
                signed_prekey: crypto::SignedPreKey {
                    id: bundle.signed_prekey_id as u32,
                    public_key: bundle.signed_prekey_public,
                    signature: bundle.signed_prekey_signature,
                },
                one_time_prekey: bundle.one_time_prekey.map(|(id, pk)| crypto::OneTimePreKey {
                    id: id as u32,
                    public_key: pk,
                }),
                kyber_prekey: crypto::prekeys::KyberPreKey {
                    id: bundle.kyber_prekey_id as u32,
                    public_key: bundle.kyber_prekey_public,
                    signature: bundle.kyber_prekey_signature,
                },
            };

            session::initiate_session(
                &mut self.store,
                &self.local_address,
                &recipient_addr,
                &recipient_bundle,
            ).await?;
        }

        let encrypted = session::encrypt(
            &mut self.store,
            &self.local_address,
            &recipient_addr,
            plaintext,
        ).await?;

        self.client.send_messages(&[OutboundMessage {
            recipient_did: recipient_did.to_string(),
            recipient_device_id: recipient_device_id as i32,
            ciphertext: encrypted.ciphertext,
            message_kind: match encrypted.kind {
                MessageKind::PreKey => 0,
                MessageKind::Whisper => 1,
            },
        }]).await?;

        Ok(())
    }

    async fn receive_messages(&mut self) -> Result<Vec<DecryptedMessage>, AppError> {
        let inbound = self.client.fetch_messages().await?;
        let mut decrypted = Vec::with_capacity(inbound.len());

        for msg in &inbound {
            decrypted.push(self.decrypt_inbound(msg).await?);
        }

        if !inbound.is_empty() {
            let ids: Vec<i64> = inbound.iter().map(|m| m.id).collect();
            self.client.ack_messages(&ids).await?;
        }

        Ok(decrypted)
    }

    async fn decrypt_inbound(
        &mut self,
        msg: &net::types::InboundMessage,
    ) -> Result<DecryptedMessage, AppError> {
        let sender_did = msg.sender_did.as_deref()
            .ok_or_else(|| AppError::Protocol("message missing sender_did".into()))?;
        let sender_device_id = msg.sender_device_id
            .ok_or_else(|| AppError::Protocol("message missing sender_device_id".into()))? as u32;

        let sender_addr = DeviceAddress::new(
            AccountId::new(sender_did),
            DeviceId::new(sender_device_id),
        );

        let encrypted = EncryptedMessage {
            ciphertext: msg.ciphertext.clone(),
            kind: if msg.message_kind == 0 { MessageKind::PreKey } else { MessageKind::Whisper },
        };

        let plaintext = session::decrypt(
            &mut self.store,
            &self.local_address,
            &sender_addr,
            &encrypted,
        ).await?;

        Ok(DecryptedMessage {
            server_id: msg.id,
            sender_did: sender_did.to_string(),
            sender_device_id,
            plaintext,
        })
    }
}
