//! Cryptographic operations for the actnet platform.
//!
//! This crate is a focused wrapper around [`libsignal_protocol`]. It does not
//! implement any cryptographic primitives itself — all key agreement, ratcheting,
//! and encryption is delegated to libsignal's audited Rust implementation.
//!
//! # Structure
//!
//! - [`identity`] — long-term identity key pairs and public keys.
//! - [`prekeys`] — one-time and signed EC prekeys plus Kyber (post-quantum)
//!   prekeys, with helpers to generate them and convert to/from wire format.
//! - [`session`] — the [`Store`] supertrait and the three session operations:
//!   [`session::initiate_session`], [`session::encrypt`], [`session::decrypt`].
//! - [`groups`] — stub for Stage 4 zkgroup anonymous credentials.
//!
//! # Design note
//!
//! `crypto` has no I/O. Session operations take a `&mut impl Store` and
//! delegate all persistence to the caller. The `store` crate provides the
//! production implementation of [`Store`]; tests can supply any compatible type.
//! This keeps cryptographic logic isolated and testable without a database.

pub mod error;
pub mod groups;
pub mod identity;
pub mod prekeys;
pub mod sealed_sender;
pub mod sender_cert;
pub mod sender_keys;
pub mod session;

pub use error::CryptoError;
pub use identity::{IdentityKey, IdentityKeyPair};
pub use prekeys::{
    GeneratedOneTimePreKey, GeneratedSignedPreKey, LocalKeyBundle, OneTimePreKey,
    RecipientKeyBundle, SignedPreKey,
};
pub use session::{DeviceAddress, EncryptedMessage, MessageKind, Store};
