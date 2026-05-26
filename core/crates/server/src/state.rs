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

/// Server-initiated WebSocket push. Variants correspond to the
/// non-response frame types in `proto/ws.proto`.
#[derive(Debug, Clone)]
pub enum WsPush {
    /// An incoming message ciphertext to deliver to the device.
    Delivery(PendingDelivery),
    /// The device's prekey pools are below threshold; client should refill.
    PrekeyLow {
        one_time_remaining: i64,
        kyber_remaining: i64,
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
}

impl AppState {
    pub fn new(db: sqlx::PgPool, config: Config) -> Self {
        Self {
            db,
            config,
            ws_connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
