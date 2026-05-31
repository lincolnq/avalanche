//! `AuthCredentialDid` — anonymous group-membership credential, DID-bound.
//!
//! Built directly on `zkcredential`, mirroring libsignal's
//! `AuthCredentialWithPniZkc`. The difference: libsignal's credential carries
//! two fixed-size (Aci, Pni) identity attributes; ours carries a single
//! variable-length DID. See `docs/03-groups.md` §2.3 for why we build our own
//! rather than packing a DID into the Aci slot (option 2 vs option 1).
//!
//! Flow:
//!
//! 1. **Issue** (server, daily): `AuthCredentialDidResponse::issue_credential`
//!    signs an `IssuanceProof` over the DID and a day-aligned redemption
//!    time, using the server's `AuthCredentialDid` key pair.
//! 2. **Receive** (client): `response.receive(did, redemption_time, public)`
//!    verifies the proof and returns the long-lived [`AuthCredentialDid`]
//!    the client stores until the credential's redemption_time elapses.
//! 3. **Present** (client, per-action): `cred.present(public, group, ...)`
//!    produces a [`AuthCredentialDidPresentation`] — a single-use proof that
//!    the holder owns a valid credential and the encrypted-member-id is the
//!    DID inside that credential, all without revealing the DID.
//! 4. **Verify** (server, per-action): `presentation.verify(secret, group,
//!    redemption_time)` checks the proof and extracts the
//!    `EncryptedMemberId` for membership-table lookup.
//!
//! What this module deliberately does NOT do: link the credential to any
//! routing identifier on the server side. The server stores
//! `member_credentials.encrypted_member_id` columns (see §3.2) and matches
//! against the verified `EncryptedMemberId` from a presentation. This module
//! produces the latter; the matching is the server's job.

use curve25519_dalek::ristretto::RistrettoPoint;
use partial_default::PartialDefault;
use poksho::{ShoApi, ShoHmacSha256};
use serde::{Deserialize, Serialize};
use zkcredential::attributes::{Attribute, Ciphertext, PublicAttribute};
use zkcredential::credentials::{Credential, CredentialKeyPair, CredentialPublicKey};
use zkcredential::issuance::{IssuanceProof, IssuanceProofBuilder};
use zkcredential::presentation::{
    PresentationProof, PresentationProofBuilder, PresentationProofVerifier,
};

use crate::error::CryptoError;
use crate::groups::group_key::{DidEncryptionDomain, GroupKey, GroupPublicParams};
use crate::groups::server_params::{ServerPublicParams, ServerSecretParams};

const CREDENTIAL_LABEL: &[u8] = b"Actnet_AuthCredentialDid_20260531";

/// A DID encoded as two Ristretto points for use as a `zkcredential`
/// attribute. Computed deterministically by hashing the DID under a fixed
/// domain separation label, then squeezing two independent points out of
/// the SHO. The DID itself is kept in the struct (alongside the points)
/// only on the client side — it never reaches the server.
#[derive(Clone, Serialize, Deserialize, PartialDefault)]
pub struct DidStruct {
    #[partial_default(value = "Vec::new()")]
    did_bytes: Vec<u8>,
    m1: RistrettoPoint,
    m2: RistrettoPoint,
}

impl DidStruct {
    pub fn from_did(did: &str) -> Self {
        let did_bytes = did.as_bytes().to_vec();
        let (m1, m2) = derive_points(&did_bytes);
        Self { did_bytes, m1, m2 }
    }

    pub fn did(&self) -> &str {
        // The struct is only ever constructed from a valid &str, so this is
        // infallible; we just don't have anywhere to return a Result-shaped
        // error.
        std::str::from_utf8(&self.did_bytes).expect("DidStruct was constructed from valid &str")
    }
}

fn derive_points(did_bytes: &[u8]) -> (RistrettoPoint, RistrettoPoint) {
    let mut sho = ShoHmacSha256::new(b"Actnet_ZKGroup_20260531_DidStruct_M1_M2");
    sho.absorb_and_ratchet(did_bytes);
    let mut m1_bytes = [0u8; 64];
    sho.squeeze_and_ratchet_into(&mut m1_bytes);
    let m1 = RistrettoPoint::from_uniform_bytes(&m1_bytes);
    let mut m2_bytes = [0u8; 64];
    sho.squeeze_and_ratchet_into(&mut m2_bytes);
    let m2 = RistrettoPoint::from_uniform_bytes(&m2_bytes);
    (m1, m2)
}

impl Attribute for DidStruct {
    fn as_points(&self) -> [RistrettoPoint; 2] {
        [self.m1, self.m2]
    }
}

/// Unix timestamp in seconds, day-aligned. We require day alignment to bound
/// the issuance domain — every credential a client holds is for a specific
/// 24-hour window, so a leaked credential can't be replayed past its day.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, PartialDefault)]
pub struct RedemptionTime(pub u64);

impl RedemptionTime {
    const SECONDS_PER_DAY: u64 = 86_400;

    pub fn from_unix_seconds(secs: u64) -> Self {
        Self(secs)
    }

    pub fn is_day_aligned(self) -> bool {
        self.0 % Self::SECONDS_PER_DAY == 0
    }
}

impl PublicAttribute for RedemptionTime {
    fn hash_into(&self, sho: &mut dyn ShoApi) {
        self.0.hash_into(sho)
    }
}

/// The server's signed response to a credential request. Wire-form returned
/// by the daily-credential-refresh endpoint.
#[derive(Clone, Serialize, Deserialize, PartialDefault)]
pub struct AuthCredentialDidResponse {
    proof: IssuanceProof,
}

/// Long-lived credential the client holds until its redemption time elapses.
#[derive(Clone, Serialize, Deserialize, PartialDefault)]
pub struct AuthCredentialDid {
    credential: Credential,
    did_struct: DidStruct,
    redemption_time: RedemptionTime,
}

/// Single-use presentation the client submits with each action. Carries the
/// proof, the encrypted-member-id (group-key-encrypted DID), and the public
/// redemption time the server matches against its policy.
#[derive(Clone, Serialize, Deserialize, PartialDefault)]
pub struct AuthCredentialDidPresentation {
    proof: PresentationProof,
    member_id_ciphertext: Ciphertext<DidEncryptionDomain>,
    redemption_time: RedemptionTime,
}

/// Server-visible encrypted DID. Opaque to the server; the
/// `member_credentials` table stores rows keyed by this. Two presentations
/// of credentials for the same DID under the same group key yield the same
/// `EncryptedMemberId` (the ciphertext is deterministic in the attribute and
/// key — randomness in the *presentation* doesn't propagate here).
#[derive(Clone, Serialize, Deserialize, PartialDefault, PartialEq, Eq)]
pub struct EncryptedMemberId(Ciphertext<DidEncryptionDomain>);

impl std::fmt::Debug for EncryptedMemberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Avoid leaking ciphertext bytes to logs by default; print only the
        // type + a hash-like length tag.
        write!(f, "EncryptedMemberId(…)")
    }
}

impl EncryptedMemberId {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.0).expect("serialize EncryptedMemberId")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        bincode::deserialize(bytes)
            .map(Self)
            .map_err(|_| CryptoError::ZkgroupDeserialize)
    }

    #[allow(dead_code)] // used by upcoming server-side membership lookup
    pub(crate) fn ciphertext(&self) -> &Ciphertext<DidEncryptionDomain> {
        &self.0
    }
}

impl AuthCredentialDidResponse {
    /// Server-side issuance. Failures here are non-recoverable bugs (e.g.
    /// non-day-aligned timestamp); we panic rather than return an error
    /// because the server has full control over both inputs.
    pub fn issue_credential(
        did: &str,
        redemption_time: RedemptionTime,
        params: &ServerSecretParams,
        randomness: [u8; zkcredential::RANDOMNESS_LEN],
    ) -> Self {
        assert!(
            redemption_time.is_day_aligned(),
            "redemption_time must be day-aligned",
        );
        Self::issue_for_key(did, redemption_time, params.auth_credential_key(), randomness)
    }

    fn issue_for_key(
        did: &str,
        redemption_time: RedemptionTime,
        key: &CredentialKeyPair,
        randomness: [u8; zkcredential::RANDOMNESS_LEN],
    ) -> Self {
        let did_struct = DidStruct::from_did(did);
        let proof = IssuanceProofBuilder::new(CREDENTIAL_LABEL)
            .add_attribute(&did_struct)
            .add_public_attribute(&redemption_time)
            .issue(key, randomness);
        Self { proof }
    }

    /// Client-side: verify the issuance proof, returning the credential to
    /// hold for the duration of `redemption_time`'s 24-hour window.
    pub fn receive(
        self,
        did: &str,
        redemption_time: RedemptionTime,
        public_params: &ServerPublicParams,
    ) -> Result<AuthCredentialDid, CryptoError> {
        self.receive_for_key(did, redemption_time, public_params.auth_credential_key())
    }

    fn receive_for_key(
        self,
        did: &str,
        redemption_time: RedemptionTime,
        public_key: &CredentialPublicKey,
    ) -> Result<AuthCredentialDid, CryptoError> {
        if !redemption_time.is_day_aligned() {
            return Err(CryptoError::InvalidCiphertext);
        }
        let did_struct = DidStruct::from_did(did);
        let credential = IssuanceProofBuilder::new(CREDENTIAL_LABEL)
            .add_attribute(&did_struct)
            .add_public_attribute(&redemption_time)
            .verify(public_key, self.proof)
            .map_err(|_| CryptoError::InvalidCiphertext)?;
        Ok(AuthCredentialDid {
            credential,
            did_struct,
            redemption_time,
        })
    }
}

impl AuthCredentialDid {
    pub fn redemption_time(&self) -> RedemptionTime {
        self.redemption_time
    }

    /// Produce a one-shot presentation bound to `group`. Each call must use
    /// fresh `randomness`; reusing it across presentations links them to the
    /// same credential and can expose the encryption key.
    pub fn present(
        &self,
        public_params: &ServerPublicParams,
        group: &GroupKey,
        randomness: [u8; zkcredential::RANDOMNESS_LEN],
    ) -> AuthCredentialDidPresentation {
        self.present_for_key(public_params.auth_credential_key(), group, randomness)
    }

    fn present_for_key(
        &self,
        public_key: &CredentialPublicKey,
        group: &GroupKey,
        randomness: [u8; zkcredential::RANDOMNESS_LEN],
    ) -> AuthCredentialDidPresentation {
        let key_pair = group.did_enc_key_pair();
        let proof = PresentationProofBuilder::new(CREDENTIAL_LABEL)
            .add_attribute(&self.did_struct, key_pair)
            .present(public_key, &self.credential, randomness);
        let member_id_ciphertext = key_pair.encrypt_arbitrary_attribute(&self.did_struct);
        AuthCredentialDidPresentation {
            proof,
            member_id_ciphertext,
            redemption_time: self.redemption_time,
        }
    }
}

impl AuthCredentialDidPresentation {
    /// Server-side: verify the proof and confirm the presented redemption
    /// time matches what the server's policy expects (typically: today, or
    /// today ± a grace window — that's the caller's choice).
    pub fn verify(
        &self,
        params: &ServerSecretParams,
        group_public: &GroupPublicParams,
        expected_redemption_time: RedemptionTime,
    ) -> Result<(), CryptoError> {
        self.verify_for_key(
            params.auth_credential_key(),
            group_public,
            expected_redemption_time,
        )
    }

    fn verify_for_key(
        &self,
        key: &CredentialKeyPair,
        group_public: &GroupPublicParams,
        expected_redemption_time: RedemptionTime,
    ) -> Result<(), CryptoError> {
        if self.redemption_time != expected_redemption_time {
            return Err(CryptoError::InvalidCiphertext);
        }
        PresentationProofVerifier::new(CREDENTIAL_LABEL)
            .add_attribute(&self.member_id_ciphertext, group_public.did_enc_public_key())
            .add_public_attribute(&self.redemption_time)
            .verify(key, &self.proof)
            .map_err(|_| CryptoError::InvalidCiphertext)
    }

    /// The encrypted-member-id this presentation binds to. The server uses
    /// this to look up the actor in the group's `member_credentials` table
    /// (after verification succeeds; never before).
    pub fn encrypted_member_id(&self) -> EncryptedMemberId {
        EncryptedMemberId(self.member_id_ciphertext)
    }

    pub fn redemption_time(&self) -> RedemptionTime {
        self.redemption_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: u64) -> RedemptionTime {
        RedemptionTime::from_unix_seconds(n * 86_400)
    }

    fn fixed_randomness(seed: u8) -> [u8; zkcredential::RANDOMNESS_LEN] {
        [seed; zkcredential::RANDOMNESS_LEN]
    }

    #[test]
    fn issue_receive_present_verify_roundtrip() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let group = GroupKey::generate();
        let did = "did:plc:abcdef1234567890abcdef12";
        let t = day(20_000);

        let response =
            AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(1));
        let credential = response.receive(did, t, &public).expect("receive ok");
        let presentation = credential.present(&public, &group, fixed_randomness(2));
        presentation.verify(&server, &group.public_params(), t).expect("verify ok");
    }

    #[test]
    fn presentations_for_same_did_under_same_group_yield_same_member_id() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let group = GroupKey::generate();
        let did = "did:plc:samedid";
        let t = day(20_000);

        let response =
            AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(3));
        let cred = response.receive(did, t, &public).expect("receive");
        let p1 = cred.present(&public, &group, fixed_randomness(4));
        let p2 = cred.present(&public, &group, fixed_randomness(5));
        // Different randomness → different proofs, but the encrypted member
        // id is a function of (DID, group key) only.
        assert_eq!(p1.encrypted_member_id(), p2.encrypted_member_id());
    }

    #[test]
    fn different_dids_yield_different_member_ids() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let group = GroupKey::generate();
        let t = day(20_000);

        let mk = |did: &str| {
            let r = AuthCredentialDidResponse::issue_credential(
                did,
                t,
                &server,
                fixed_randomness(6),
            );
            r.receive(did, t, &public)
                .unwrap()
                .present(&public, &group, fixed_randomness(7))
                .encrypted_member_id()
        };
        assert_ne!(mk("did:plc:alice"), mk("did:plc:bob"));
    }

    #[test]
    fn same_did_under_different_groups_yields_different_member_ids() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let g1 = GroupKey::generate();
        let g2 = GroupKey::generate();
        let did = "did:plc:alice";
        let t = day(20_000);

        let r = AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(8));
        let cred = r.receive(did, t, &public).unwrap();
        let m1 = cred
            .present(&public, &g1, fixed_randomness(9))
            .encrypted_member_id();
        let m2 = cred
            .present(&public, &g2, fixed_randomness(10))
            .encrypted_member_id();
        assert_ne!(m1, m2);
    }

    #[test]
    fn receive_rejects_wrong_did() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let t = day(20_000);

        let response = AuthCredentialDidResponse::issue_credential(
            "did:plc:alice",
            t,
            &server,
            fixed_randomness(11),
        );
        // Client claims to receive for a different DID — issuance proof must
        // fail.
        assert!(response.receive("did:plc:mallory", t, &public).is_err());
    }

    #[test]
    fn receive_rejects_wrong_redemption_time() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let did = "did:plc:alice";

        let response = AuthCredentialDidResponse::issue_credential(
            did,
            day(20_000),
            &server,
            fixed_randomness(12),
        );
        assert!(response.receive(did, day(20_001), &public).is_err());
    }

    #[test]
    fn receive_rejects_non_day_aligned_redemption() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let did = "did:plc:alice";
        let t = day(20_000);
        let r = AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(13));
        // Construct an awkward non-aligned time; receive must refuse it.
        let bad = RedemptionTime::from_unix_seconds(20_000 * 86_400 + 1);
        assert!(r.receive(did, bad, &public).is_err());
    }

    #[test]
    fn verify_rejects_wrong_group() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let g1 = GroupKey::generate();
        let g2 = GroupKey::generate();
        let did = "did:plc:alice";
        let t = day(20_000);

        let response =
            AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(14));
        let cred = response.receive(did, t, &public).unwrap();
        let presentation = cred.present(&public, &g1, fixed_randomness(15));
        assert!(presentation.verify(&server, &g2.public_params(), t).is_err());
    }

    #[test]
    fn verify_rejects_wrong_redemption_time() {
        let server = ServerSecretParams::generate();
        let public = server.public_params();
        let group = GroupKey::generate();
        let did = "did:plc:alice";
        let t = day(20_000);

        let response =
            AuthCredentialDidResponse::issue_credential(did, t, &server, fixed_randomness(16));
        let cred = response.receive(did, t, &public).unwrap();
        let presentation = cred.present(&public, &group, fixed_randomness(17));
        // Server expected a different day → reject.
        assert!(presentation.verify(&server, &group.public_params(), day(20_001)).is_err());
    }

    #[test]
    fn verify_rejects_wrong_server_key() {
        let server_a = ServerSecretParams::generate();
        let server_b = ServerSecretParams::generate();
        let public = server_a.public_params();
        let group = GroupKey::generate();
        let did = "did:plc:alice";
        let t = day(20_000);

        let response =
            AuthCredentialDidResponse::issue_credential(did, t, &server_a, fixed_randomness(18));
        let cred = response.receive(did, t, &public).unwrap();
        let presentation = cred.present(&public, &group, fixed_randomness(19));
        // A different server can't validate a credential issued by server_a.
        assert!(presentation.verify(&server_b, &group.public_params(), t).is_err());
    }
}
