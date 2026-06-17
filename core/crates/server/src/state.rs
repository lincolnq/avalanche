//! Shared application state available to all request handlers.
//!
//! [`AppState`] is cloned into every Axum handler via the `State` extractor.
//! It holds the database pool, server config, and the in-memory WebSocket
//! connection map. The connection map tracks which devices currently have a
//! live WebSocket so that incoming messages can be pushed immediately rather
//! than waiting for the client to poll.
//!
//! # Scaling note
//!
//! The WebSocket connection map is in-process (`Arc<RwLock<HashMap>>`). This
//! works for a single server instance. For horizontal scaling, the map would
//! need to be backed by PostgreSQL `LISTEN/NOTIFY` or a shared pub/sub layer
//! so that a message enqueued on instance A can notify a WebSocket on
//! instance B.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crypto::groups::ServerSecretParams;
use crypto::sender_cert::SenderCertChain;

use crate::config::Config;

/// A pending message delivery to push to a connected device. The WebSocket
/// handler allocates a frame correlation id, records `message_id` against
/// it, encodes a `DeliverRequest` protobuf frame, and sends it. The server
/// removes the queued message only after receiving the matching `DeliverAck`.
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    pub message_id: i64,
    pub ciphertext: Vec<u8>,
    pub message_kind: i16,
    pub sender_did: Option<String>,
    pub sender_device_id: Option<i32>,
    pub enqueued_at: Option<String>,
}

/// A pending group-message delivery to push to a subscribed pseudonym.
/// Carries the per-recipient SSv2 ReceivedMessage slice produced server-
/// side by parsing the sender's multi-recipient envelope.
#[derive(Debug, Clone)]
pub struct PendingGroupDelivery {
    pub message_id: i64,
    pub group_id: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub recipient_group_pseudonym: Vec<u8>,
    pub enqueued_at: Option<String>,
}

/// Server-initiated WebSocket push. Variants correspond to the
/// non-response frame types in `proto/ws.proto`.
#[derive(Debug, Clone)]
pub enum WsPush {
    /// An incoming 1:1 message ciphertext to deliver to the device.
    Delivery(PendingDelivery),
    /// An incoming sealed-sender group message slice to deliver to a
    /// subscribed `recipient_group_pseudonym`.
    GroupDelivery(PendingGroupDelivery),
    /// The device's prekey pools are below threshold; client should refill.
    PrekeyLow {
        one_time_remaining: i64,
        kyber_remaining: i64,
    },
    /// A new account just registered on this server. Pushed to every
    /// connected bot session that holds the `subscribe.account_joined`
    /// capability (see [`AppState::account_joined_subscribers`]).
    AccountJoined {
        did: String,
        joined_at_ms: i64,
        /// The raw invite token the account registered with, if any. Carries
        /// the issuer stamp + routing tags a bot uses to route the new member
        /// (docs/22 join-event API, docs/24 post-join hand-off).
        invite_token: Option<String>,
    },
}

/// Shared application state, available to all request handlers via Axum's
/// State extractor.
#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: Config,
    /// Connected WebSocket devices: internal device PK -> sender channel.
    pub ws_connections: Arc<RwLock<HashMap<i64, mpsc::UnboundedSender<WsPush>>>>,
    /// Group-message subscriptions: `recipient_group_pseudonym` (raw bytes)
    /// -> the sender channel of the device that subscribed. A device with
    /// memberships in N groups carries N entries here. Population is
    /// driven by the WS `SubscribeGroupPseudonyms` frame; entries are
    /// dropped when the socket closes.
    pub group_subscriptions: Arc<RwLock<HashMap<Vec<u8>, mpsc::UnboundedSender<WsPush>>>>,
    /// The homeserver's zkgroup signing key, loaded once at startup and held
    /// in memory thereafter. Used to issue auth credentials and group send
    /// endorsements; never transmitted off the server.
    pub zkgroup_secret: Arc<ServerSecretParams>,
    /// The homeserver's sender-cert chain, loaded once at startup. Used to
    /// sign per-message `SenderCertificate`s in the sealed-sender group
    /// flow. Trust root pubkey is published via `/v1/groups/server-params`.
    pub sender_cert_chain: Arc<SenderCertChain>,
    /// Connected bot sessions that hold the `subscribe.account_joined`
    /// capability: internal device PK -> sender channel. Populated by the WS
    /// handler at connect time after resolving the account's capabilities
    /// (the pinned adminbot Project's bots get it via the superuser
    /// short-circuit); cleared on disconnect. `AccountJoinedEvent` pushes fan
    /// out to every entry. Live pushes are best-effort — durable catch-up is
    /// via `server_events` + `GET /v1/admin/events`, so a disconnected bot
    /// misses nothing.
    pub account_joined_subscribers: Arc<RwLock<HashMap<i64, mpsc::UnboundedSender<WsPush>>>>,
}

impl AppState {
    pub fn new(
        db: sqlx::PgPool,
        config: Config,
        zkgroup_secret: ServerSecretParams,
        sender_cert_chain: SenderCertChain,
    ) -> Self {
        Self {
            db,
            config,
            ws_connections: Arc::new(RwLock::new(HashMap::new())),
            group_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            zkgroup_secret: Arc::new(zkgroup_secret),
            sender_cert_chain: Arc::new(sender_cert_chain),
            account_joined_subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
