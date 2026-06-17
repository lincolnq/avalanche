//! Database access layer for the homeserver.
//!
//! Each submodule corresponds to a table or group of related tables in the
//! PostgreSQL schema. All functions take a `&mut PgConnection` and return
//! `sqlx::Error`. This allows callers to pass either a pool connection
//! (auto-commit) or a transaction (for atomicity or test rollback).
//! The routes layer wraps these in [`crate::error::ServerError`] via the
//! `From<sqlx::Error>` impl.
//!
//! # Security note
//!
//! The server database stores only public key material and opaque ciphertext.
//! Private keys never reach the server. Message content columns are `bytea`
//! blobs that the server cannot decrypt. A database dump reveals routing
//! metadata (who messaged whom, when, message sizes) but no plaintext.

pub mod abuse;
pub mod accounts;
pub mod capabilities;
pub mod challenges;
pub mod devices;
pub mod did;
pub mod groups;
pub mod group_messages;
pub mod messages;
pub mod prekeys;
pub mod profiles;
pub mod projects;
pub mod project_tokens;
pub mod push;
pub mod ip_rate_limits;
pub mod rate_limits;
pub mod server_events;
pub mod sessions;
pub mod storage;
pub mod token_redemptions;
pub mod zkgroup_params;
