//! On-device encrypted storage for the actnet mobile client.
//!
//! This crate owns everything that touches the local SQLCipher database. It has
//! two distinct responsibilities:
//!
//! 1. **Implement [`crypto::Store`]** — the libsignal store traits that the
//!    `crypto` crate requires for session state persistence. Every encrypt and
//!    decrypt call mutates session records, prekey records, and identity keys;
//!    this crate persists those changes so they survive across app launches.
//!
//! 2. **Provide its own storage API** — account identity, registration state,
//!    the outbound message queue, and the prekey pools are all managed here,
//!    outside the scope of libsignal's trait definitions.
//!
//! # Structure
//!
//! - [`db`] — the [`Store`] handle and [`DatabaseKey`]; the entry point for
//!   opening or creating the database.
//! - [`schema`] — SQL migration constants applied on every open.
//! - [`session`] — implementations of libsignal's `SessionStore`,
//!   `IdentityKeyStore`, `PreKeyStore`, `SignedPreKeyStore`, and
//!   `KyberPreKeyStore` traits, plus the [`crypto::Store`] blanket impl.
//! - [`account`] — save and load the local identity key pair and registration
//!   details.
//! - [`prekeys`] — manage the one-time EC and Kyber prekey pools; track
//!   remaining counts so `app-core` knows when to refill.
//! - [`messages`] — the outbound message queue for messages that could not be
//!   delivered immediately.
//! - [`groups`] — stub for Stage 4 group state and credential storage.
//!
//! # Database encryption
//!
//! The database file is encrypted with AES-256 via SQLCipher. The key is
//! supplied through [`DatabaseKey`]. In Stage 1 a placeholder key is used; in
//! Stage 3 the key is derived from a secret held in the iOS Secure Enclave or
//! Android Keystore so the file is useless without the device's hardware.

pub mod account;
pub mod db;
pub mod error;
pub mod groups;
pub mod messages;
pub mod prekeys;
pub mod profiles;
pub mod push;
pub mod schema;
pub mod session;

pub use db::{DatabaseKey, Store};
pub use error::StoreError;
