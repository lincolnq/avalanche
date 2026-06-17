//! The actnet homeserver.
//!
//! This crate is the HTTP/WebSocket server that activist organizations run.
//! It handles account registration, prekey distribution, encrypted message
//! relay, and real-time delivery — but it never sees plaintext. Every message
//! content column in the database is opaque `bytea` ciphertext encrypted by
//! the client's Signal-protocol stack.
//!
//! # Structure
//!
//! - [`config`] — server configuration loaded from environment variables.
//! - [`state`] — shared application state: database pool, config, WebSocket
//!   connection map.
//! - [`error`] — `ServerError` enum with Axum `IntoResponse` impl.
//! - [`db`] — database access layer: accounts, devices, DID documents,
//!   session tokens, prekeys, and the encrypted message queue.
//! - [`routes`] — HTTP and WebSocket endpoints (registration, auth, prekey
//!   upload/fetch, message send/fetch/ack, DID resolution, WebSocket).
//! - [`middleware`] — request extractors: session-token authentication.
//! - [`tasks`] — background cleanup: expired messages and session tokens.
//!
//! # Security posture
//!
//! The server is explicitly untrusted with respect to message content. It
//! stores and forwards ciphertext it cannot read. The threat model targets
//! **server seizure**: if this binary and its database are captured, the
//! attacker learns account metadata (DIDs, device registrations, message
//! timing) but no message content, group memberships beyond what the server
//! mediates, or private key material.
//!
//! The server *does* have access to:
//! - Account DIDs and device public identity keys
//! - Message routing metadata (who sent to whom, when, message sizes)
//! - Prekey material (public halves only)
//! - Session token validity
//!
//! It does *not* have access to:
//! - Message plaintext (encrypted client-side before submission)
//! - Private keys (never leave the client device)
//! - Group membership lists (managed client-side in encrypted group state)

pub mod config;
pub mod db;
pub mod error;
pub mod invite_token;
pub mod middleware;
pub mod migrate;
pub mod plc;
pub mod routes;
pub mod state;
pub mod tasks;

/// Generated protobuf types for the `/v1/ws` framing.
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/actnet.ws.rs"));
}
