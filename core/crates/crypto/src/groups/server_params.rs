//! Per-homeserver zkgroup signing material.
//!
//! Each homeserver holds exactly one [`ServerSecretParams`] — the signing
//! material it uses to issue anonymous auth credentials and (Stage 6 PR 2)
//! group send endorsements. The matching [`ServerPublicParams`] is
//! published so clients can verify issuances and produce presentations.
//! Persistence (DB storage, hot-reload on startup) is the server's job;
//! this module only provides the crypto-level primitives.
//!
//! These are thin newtype wrappers around `zkgroup::ServerSecretParams` /
//! `zkgroup::ServerPublicParams`. See `docs/03-groups.md` §2.3 / §2.4 for
//! why we use stock zkgroup types rather than a parallel DID-shaped
//! scheme: identities are carried as `Aci::from(UUID(did))`, so every
//! zkgroup primitive works as designed.
//!
//! Wire format is bincode-serialized zkgroup bytes. Stable for a given
//! pinned libsignal commit. Changing it requires a migration; see
//! `infra/migrations/009_zkgroup_server_params.sql`.

use rand::TryRngCore;

use crate::error::CryptoError;

/// Server-side signing material. Holding this is equivalent to holding
/// the homeserver's authority to issue auth credentials and group send
/// endorsements.
#[derive(Clone)]
pub struct ServerSecretParams {
    inner: zkgroup::ServerSecretParams,
}

/// Public counterpart published to clients so they can verify credential
/// issuances and produce presentations.
#[derive(Clone)]
pub struct ServerPublicParams {
    inner: zkgroup::ServerPublicParams,
}

fn fresh_randomness() -> [u8; zkgroup::RANDOMNESS_LEN] {
    let mut r = [0u8; zkgroup::RANDOMNESS_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut r)
        .expect("OS RNG failed");
    r
}

impl ServerSecretParams {
    pub fn generate() -> Self {
        Self {
            inner: zkgroup::ServerSecretParams::generate(fresh_randomness()),
        }
    }

    pub fn public_params(&self) -> ServerPublicParams {
        ServerPublicParams {
            inner: self.inner.get_public_params(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        zkgroup::serialize(&self.inner)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let inner = zkgroup::deserialize::<zkgroup::ServerSecretParams>(bytes)
            .map_err(|_| CryptoError::ZkgroupDeserialize)?;
        Ok(Self { inner })
    }

    /// Borrow the underlying zkgroup secret. Used by credential
    /// issuance and (Stage 6 PR 2) endorsement issuance, both of which
    /// take `&zkgroup::ServerSecretParams`.
    pub fn zkgroup(&self) -> &zkgroup::ServerSecretParams {
        &self.inner
    }
}

impl ServerPublicParams {
    pub fn to_bytes(&self) -> Vec<u8> {
        zkgroup::serialize(&self.inner)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let inner = zkgroup::deserialize::<zkgroup::ServerPublicParams>(bytes)
            .map_err(|_| CryptoError::ZkgroupDeserialize)?;
        Ok(Self { inner })
    }

    pub fn zkgroup(&self) -> &zkgroup::ServerPublicParams {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_roundtrip_secret() {
        let secret = ServerSecretParams::generate();
        let bytes = secret.to_bytes();
        let decoded = ServerSecretParams::from_bytes(&bytes).expect("decode");
        assert_eq!(bytes, decoded.to_bytes());
    }

    #[test]
    fn public_params_derive_and_roundtrip() {
        let secret = ServerSecretParams::generate();
        let public = secret.public_params();
        let bytes = public.to_bytes();
        let decoded = ServerPublicParams::from_bytes(&bytes).expect("decode");
        assert_eq!(bytes, decoded.to_bytes());
    }

    #[test]
    fn from_bytes_rejects_garbage() {
        assert!(ServerSecretParams::from_bytes(&[0u8; 8]).is_err());
        assert!(ServerPublicParams::from_bytes(&[0u8; 8]).is_err());
    }

    #[test]
    fn distinct_generates_differ() {
        let a = ServerSecretParams::generate();
        let b = ServerSecretParams::generate();
        assert_ne!(a.to_bytes(), b.to_bytes());
    }
}
