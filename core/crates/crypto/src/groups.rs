//! Anonymous credentials and encrypted-state primitives for action-bound
//! groups. Thin wrappers around libsignal's `zkgroup` crate.
//!
//! See `docs/03-groups.md` for the design. Per §2.3 we carry identities
//! as `Aci::from(UUID(did))` so every zkgroup primitive works as
//! designed — no parallel DID-shaped scheme. This module is the
//! scheme-agnostic boundary between `app-core` / `server` and the
//! underlying zkgroup primitives, so that swapping in MLS later
//! requires no changes outside this module.

pub mod group_key;
pub mod server_params;

pub use group_key::{did_to_uuid, EncryptedMemberId, GroupId, GroupKey, GroupPublicParams};
pub use server_params::{ServerPublicParams, ServerSecretParams};

// Re-export the stock zkgroup credential types used by app-core and
// server. Callers don't need to depend on `zkgroup` directly for the
// happy path; they can if they need lower-level access via the
// `zkgroup_*` accessors on our wrapper types.
pub use zkgroup::auth::{
    AuthCredentialWithPniZkc, AuthCredentialWithPniZkcPresentation,
    AuthCredentialWithPniZkcResponse,
};
pub use zkgroup::Timestamp as RedemptionTime;
