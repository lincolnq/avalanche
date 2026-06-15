//! Group send endorsements — wrappers around `zkgroup::groups::GroupSend*`.
//!
//! Endorsements let the homeserver authorize a member to send a group message
//! to specific recipients *without* re-deriving their identity. Daily flow:
//!
//! 1. **Server** issues a `GroupSendEndorsementsResponse` over the group's
//!    full member-ciphertext list (valid until end-of-next-day,
//!    see [`default_expiration_unix_seconds`]). The response is the same for
//!    every member of a given group, so it can be cached server-side.
//! 2. **Client** validates the response with [`receive_endorsements`] and
//!    stores one per-member endorsement keyed by DID.
//! 3. **Sender** picks the recipient set (everyone except self), combines
//!    their endorsements and produces a `GroupSendFullToken` via
//!    [`token_for_recipients`].
//! 4. **Chat server** (or our `/v1/groups/{id}/send` endpoint) verifies the
//!    token authorizes sending to those DIDs via [`verify_token`].
//!
//! All data crosses the wire as opaque serialized bytes; the wrapper hides
//! the underlying zkgroup types so callers don't need to depend on `zkgroup`
//! directly.

use libsignal_core::Aci;
use rand::rngs::OsRng;
use rand::TryRngCore as _;
use zkgroup::Timestamp as ZkTimestamp;

use crate::error::CryptoError;
use crate::groups::{did_to_uuid, GroupKey, ServerPublicParams, ServerSecretParams};

/// Default endorsement expiration: end of the day after tomorrow (rounded up
/// when "tomorrow" is less than 25 hours away). Matches Signal's heuristic so
/// the issuance window is at least ~25h and at most ~49h.
pub fn default_expiration_unix_seconds(now_unix_seconds: u64) -> u64 {
    let now = ZkTimestamp::from_epoch_seconds(now_unix_seconds);
    zkgroup::groups::GroupSendEndorsementsResponse::default_expiration(now).epoch_seconds()
}

/// Server-side: issue endorsements for every member of the group. Member
/// ciphertexts must be the serialized [`crate::groups::EncryptedMemberId`]s
/// the server already has on file; order is irrelevant (zkgroup canonicalizes
/// internally), so callers can hand them over in storage order.
///
/// `expiration_unix_seconds` must be day-aligned and within zkgroup's window
/// (≥2h and ≤7d from now); using [`default_expiration_unix_seconds`] is the
/// safe choice.
pub fn issue_endorsements(
    server_secret: &ServerSecretParams,
    member_ciphertext_bytes: &[Vec<u8>],
    expiration_unix_seconds: u64,
) -> Result<Vec<u8>, CryptoError> {
    let mut ciphertexts = Vec::with_capacity(member_ciphertext_bytes.len());
    for bytes in member_ciphertext_bytes {
        let ct: zkgroup::groups::UuidCiphertext =
            zkgroup::deserialize(bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
        ciphertexts.push(ct);
    }
    let expiration = ZkTimestamp::from_epoch_seconds(expiration_unix_seconds);
    let key_pair = zkgroup::groups::GroupSendDerivedKeyPair::for_expiration(
        expiration,
        server_secret.zkgroup(),
    );
    let mut randomness = [0u8; zkgroup::RANDOMNESS_LEN];
    OsRng
        .try_fill_bytes(&mut randomness)
        .expect("OS RNG failed");
    let response = zkgroup::groups::GroupSendEndorsementsResponse::issue(
        ciphertexts,
        &key_pair,
        randomness,
    );
    Ok(zkgroup::serialize(&response))
}

/// Client-side: validate a server-issued endorsement response and return one
/// serialized endorsement per member, in the order given by `member_dids`.
/// `member_dids` must contain every member of the group (including the
/// caller) — zkgroup needs the full set to verify, since the underlying MAC
/// is over the sorted ciphertext list.
pub fn receive_endorsements(
    response_bytes: &[u8],
    member_dids: &[String],
    group_key: &GroupKey,
    server_public: &ServerPublicParams,
    now_unix_seconds: u64,
) -> Result<Vec<Vec<u8>>, CryptoError> {
    let response: zkgroup::groups::GroupSendEndorsementsResponse =
        zkgroup::deserialize(response_bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
    let now = ZkTimestamp::from_epoch_seconds(now_unix_seconds);
    let service_ids: Vec<libsignal_core::ServiceId> = member_dids
        .iter()
        .map(|did| Aci::from(did_to_uuid(did)).into())
        .collect();
    let received = response
        .receive_with_service_ids_single_threaded(
            service_ids,
            now,
            group_key.zkgroup_secret(),
            server_public.zkgroup(),
        )
        .map_err(|_| CryptoError::InvalidCiphertext)?;
    // Serialize the decompressed form so callers can deserialize back to
    // `GroupSendEndorsement` (default storage = `RistrettoPoint`) without
    // having to spell out the Signal-fork curve25519 type path.
    Ok(received
        .into_iter()
        .map(|r| zkgroup::serialize(&r.decompressed))
        .collect())
}

/// Client-side: validate a server-issued endorsement response using the group's
/// member **ciphertexts** (serialized [`crate::groups::EncryptedMemberId`]s)
/// rather than member DIDs, returning one serialized endorsement per ciphertext
/// **in the given order**.
///
/// This is the correct primitive whenever the caller may not know every
/// member's DID — e.g. members admitted via an invite link (`approve_join_request`
/// stores them with an empty DID until they reveal it). The endorsement MAC is
/// over the *full* member set, so [`receive_endorsements`] (which derives the set
/// from DIDs) fails the moment any member's DID is unknown; the EMIs, by
/// contrast, are present in cached group state for every member.
pub fn receive_endorsements_by_ciphertexts(
    response_bytes: &[u8],
    member_ciphertext_bytes: &[Vec<u8>],
    server_public: &ServerPublicParams,
    now_unix_seconds: u64,
) -> Result<Vec<Vec<u8>>, CryptoError> {
    let response: zkgroup::groups::GroupSendEndorsementsResponse =
        zkgroup::deserialize(response_bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
    let now = ZkTimestamp::from_epoch_seconds(now_unix_seconds);
    let mut ciphertexts = Vec::with_capacity(member_ciphertext_bytes.len());
    for bytes in member_ciphertext_bytes {
        let ct: zkgroup::groups::UuidCiphertext =
            zkgroup::deserialize(bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
        ciphertexts.push(ct);
    }
    let received = response
        .receive_with_ciphertexts(ciphertexts, now, server_public.zkgroup())
        .map_err(|_| CryptoError::InvalidCiphertext)?;
    // Result is in the same order as `member_ciphertext_bytes`.
    Ok(received
        .into_iter()
        .map(|r| zkgroup::serialize(&r.decompressed))
        .collect())
}

/// Client-side: combine the supplied endorsements and produce a
/// `GroupSendFullToken` authorizing sending to that set of recipients.
/// Caller supplies the endorsement bytes for the intended recipients (e.g.
/// everyone in the group except self) and the same `expiration` that the
/// server originally issued the endorsements with.
pub fn token_for_recipients(
    endorsement_bytes: &[Vec<u8>],
    group_key: &GroupKey,
    expiration_unix_seconds: u64,
) -> Result<Vec<u8>, CryptoError> {
    if endorsement_bytes.is_empty() {
        return Err(CryptoError::InvalidCiphertext);
    }
    let mut endorsements = Vec::with_capacity(endorsement_bytes.len());
    for bytes in endorsement_bytes {
        let endorsement: zkgroup::groups::GroupSendEndorsement =
            zkgroup::deserialize(bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
        endorsements.push(endorsement);
    }
    let combined = zkgroup::groups::GroupSendEndorsement::combine(endorsements);
    let token = combined.to_token(group_key.zkgroup_secret());
    let full = token.into_full_token(ZkTimestamp::from_epoch_seconds(expiration_unix_seconds));
    Ok(zkgroup::serialize(&full))
}

/// Server-side (chat-relay): verify that `token_bytes` authorizes sending a
/// group message to `recipient_dids` at `now_unix_seconds`. Returns `Ok(())`
/// on success; otherwise an error (e.g. expired token, wrong recipient set,
/// MAC failure).
pub fn verify_token(
    token_bytes: &[u8],
    recipient_dids: &[String],
    server_secret: &ServerSecretParams,
    now_unix_seconds: u64,
) -> Result<(), CryptoError> {
    let service_ids: Vec<libsignal_core::ServiceId> = recipient_dids
        .iter()
        .map(|did| Aci::from(did_to_uuid(did)).into())
        .collect();
    verify_token_for_service_ids(token_bytes, &service_ids, server_secret, now_unix_seconds)
}

/// Same as [`verify_token`] but takes `ServiceId`s directly — used by the
/// `/v1/groups/{id}/send` endpoint, which extracts them from the sealed-
/// sender envelope's recipient fanout. The server never has DIDs there.
pub fn verify_token_for_service_ids(
    token_bytes: &[u8],
    recipient_service_ids: &[libsignal_core::ServiceId],
    server_secret: &ServerSecretParams,
    now_unix_seconds: u64,
) -> Result<(), CryptoError> {
    let token: zkgroup::groups::GroupSendFullToken =
        zkgroup::deserialize(token_bytes).map_err(|_| CryptoError::ZkgroupDeserialize)?;
    let key_pair = zkgroup::groups::GroupSendDerivedKeyPair::for_expiration(
        token.expiration(),
        server_secret.zkgroup(),
    );
    let now = ZkTimestamp::from_epoch_seconds(now_unix_seconds);
    token
        .verify(recipient_service_ids.iter().copied(), now, &key_pair)
        .map_err(|_| CryptoError::InvalidCiphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_seconds() -> u64 {
        // Pick a fixed time so default_expiration is deterministic in tests
        // (day-aligned, ≥25h ahead).
        1_700_000_000
    }

    fn setup() -> (
        ServerSecretParams,
        ServerPublicParams,
        GroupKey,
        Vec<String>,
        Vec<Vec<u8>>,
    ) {
        let secret = ServerSecretParams::generate();
        let public = secret.public_params();
        let group = GroupKey::generate();
        let dids: Vec<String> = (0..3).map(|i| format!("did:plc:user-{i}")).collect();
        let ciphertexts: Vec<Vec<u8>> = dids
            .iter()
            .map(|d| zkgroup::serialize(&group.encrypt_member_id(d)))
            .collect();
        (secret, public, group, dids, ciphertexts)
    }

    #[test]
    fn issue_receive_token_verify_roundtrip() {
        let (secret, public, group, dids, ciphertexts) = setup();
        let now = now_seconds();
        let expiration = default_expiration_unix_seconds(now);

        let response = issue_endorsements(&secret, &ciphertexts, expiration).expect("issue");
        let endorsements =
            receive_endorsements(&response, &dids, &group, &public, now).expect("receive");
        assert_eq!(endorsements.len(), dids.len());

        // Sender is dids[0]; build token to recipients dids[1..].
        let recipients: Vec<String> = dids[1..].to_vec();
        let recipient_endorsements: Vec<Vec<u8>> = endorsements[1..].to_vec();
        let token = token_for_recipients(&recipient_endorsements, &group, expiration)
            .expect("token");

        verify_token(&token, &recipients, &secret, now).expect("verify");
    }

    #[test]
    fn receive_by_ciphertexts_matches_did_path_and_verifies() {
        let (secret, public, group, dids, ciphertexts) = setup();
        let now = now_seconds();
        let expiration = default_expiration_unix_seconds(now);
        let response = issue_endorsements(&secret, &ciphertexts, expiration).expect("issue");

        // Validating over the member ciphertexts yields endorsements aligned to
        // the input order (== `dids` order here), usable without knowing DIDs.
        let by_ct = receive_endorsements_by_ciphertexts(&response, &ciphertexts, &public, now)
            .expect("receive by ciphertexts");
        assert_eq!(by_ct.len(), ciphertexts.len());

        // The recipient subset (drop sender dids[0]) builds a token that the
        // server verifies against the recipient DIDs.
        let token = token_for_recipients(&by_ct[1..].to_vec(), &group, expiration).expect("token");
        verify_token(&token, &dids[1..], &secret, now).expect("verify");
    }

    #[test]
    fn verify_rejects_wrong_recipient_set() {
        let (secret, public, group, dids, ciphertexts) = setup();
        let now = now_seconds();
        let expiration = default_expiration_unix_seconds(now);
        let response = issue_endorsements(&secret, &ciphertexts, expiration).expect("issue");
        let endorsements =
            receive_endorsements(&response, &dids, &group, &public, now).expect("receive");
        // Token covers dids[1..], but we'll try to verify it against dids[0..].
        let token = token_for_recipients(&endorsements[1..].to_vec(), &group, expiration)
            .expect("token");
        assert!(verify_token(&token, &dids[..2], &secret, now).is_err());
    }

    #[test]
    fn verify_rejects_after_expiration() {
        let (secret, public, group, dids, ciphertexts) = setup();
        let now = now_seconds();
        let expiration = default_expiration_unix_seconds(now);
        let response = issue_endorsements(&secret, &ciphertexts, expiration).expect("issue");
        let endorsements =
            receive_endorsements(&response, &dids, &group, &public, now).expect("receive");
        let token =
            token_for_recipients(&endorsements[1..].to_vec(), &group, expiration).expect("token");
        // Jump well past expiration.
        let later = expiration + 1;
        assert!(verify_token(&token, &dids[1..], &secret, later).is_err());
    }
}
