//! Anonymous credentials and encrypted-state primitives for action-bound
//! groups. Wraps libsignal's `zkgroup` and `zkcredential` crates.
//!
//! See `docs/03-groups.md` for the design. This module is the scheme-agnostic
//! boundary between `app-core` / `server` and the underlying zkgroup
//! primitives, so that swapping in MLS later requires no changes outside this
//! module.

pub mod credentials;
pub mod group_key;
pub mod server_params;

pub use credentials::{
    AuthCredentialDid, AuthCredentialDidPresentation, AuthCredentialDidResponse,
    EncryptedMemberId, RedemptionTime,
};
pub use group_key::{GroupId, GroupKey, GroupPublicParams};
pub use server_params::{ServerPublicParams, ServerSecretParams};
