//! Sealed Sender v2 envelope for group messages.
//!
//! Thin wrappers around libsignal's `sealed_sender_multi_recipient_encrypt` /
//! `sealed_sender_decrypt_to_usmc` and the server-side `SealedSenderV2SentMessage`
//! parser. We only expose the SSv2 multi-recipient flow — 1:1 sealed sender is
//! not used in actnet, since DMs already go through ordinary Double Ratchet.
//!
//! Group-message flow:
//! 1. Sender derives a SenderKey ciphertext via `crypto::sender_keys::group_encrypt`.
//! 2. Sender wraps it in a `UnidentifiedSenderMessageContent` (type=SenderKey)
//!    plus a [`SenderCertificate`], then fans out per recipient device via
//!    [`encrypt_group_envelope`]. Each device's session record is loaded from
//!    the `Store` (the same DM sessions established at invite time).
//! 3. Server parses the resulting SentMessage with [`parse_sent_message`] and
//!    delivers each recipient's `ReceivedMessage` slice.
//! 4. Recipient calls [`decrypt_envelope_to_usmc`] to extract the inner
//!    SenderKey ciphertext + sender cert; the caller validates the cert against
//!    the pinned trust root (`crypto::sender_cert::validate_sender_cert`) and
//!    then feeds the contents to `group_decrypt`.

use libsignal_protocol::{
    self as signal, CiphertextMessageType, ContentHint, ProtocolAddress, SenderCertificate,
    UnidentifiedSenderMessageContent,
};
use rand::rngs::OsRng;
use rand::TryRngCore as _;

use crate::{error::CryptoError, session::Store};

pub use libsignal_protocol::ProtocolAddress as Destination;

/// Encrypt a SenderKey ciphertext into a Sealed Sender v2 SentMessage addressed
/// to all of `destinations`. Each destination's session is loaded from `store`;
/// the destination's `name()` must be a parseable `ServiceId` string (UUID for
/// ACI, prefixed UUID for PNI), since SSv2 encodes ServiceIds on the wire.
pub async fn encrypt_group_envelope<S: Store>(
    store: &mut S,
    sender_cert_bytes: &[u8],
    group_id: Option<Vec<u8>>,
    sender_key_ciphertext: &[u8],
    destinations: &[ProtocolAddress],
) -> Result<Vec<u8>, CryptoError> {
    let sender_cert = SenderCertificate::deserialize(sender_cert_bytes)?;

    let usmc = UnidentifiedSenderMessageContent::new(
        CiphertextMessageType::SenderKey,
        sender_cert,
        sender_key_ciphertext.to_vec(),
        ContentHint::Default,
        group_id,
    )?;

    // Load each device's SessionRecord. We need the records by-value so we can
    // hand &[&SessionRecord] to libsignal without aliasing the store.
    let mut sessions = Vec::with_capacity(destinations.len());
    for dest in destinations {
        let rec = store
            .load_session(dest)
            .await?
            .ok_or_else(|| CryptoError::NoSession(format!("{dest}")))?;
        sessions.push(rec);
    }
    let dest_refs: Vec<&ProtocolAddress> = destinations.iter().collect();
    let session_refs: Vec<&signal::SessionRecord> = sessions.iter().collect();

    let mut identity_store = store.clone();
    let mut rng = OsRng.unwrap_err();
    let bytes = signal::sealed_sender_multi_recipient_encrypt(
        &dest_refs,
        &session_refs,
        std::iter::empty::<signal::ServiceId>(),
        &usmc,
        &mut identity_store,
        &mut rng,
    )
    .await?;
    Ok(bytes)
}

/// One recipient's fan-out slot in a parsed SentMessage.
#[derive(Debug, Clone)]
pub struct RecipientFanout {
    /// `ServiceId` in 17-byte fixed-width-binary form.
    pub service_id_fixed_width: [u8; 17],
    /// `(device_id, registration_id)` pairs for this recipient.
    pub devices: Vec<(u32, u16)>,
    /// The exact bytes to deliver to this recipient as a SSv2 ReceivedMessage.
    /// Built by concatenating `[version, recipient_key_material, shared_bytes]`.
    pub received_message: Vec<u8>,
}

/// Server-side: parse a SentMessage produced by [`encrypt_group_envelope`] into
/// per-recipient delivery slices. The server uses these to fan the message out
/// over its delivery channels (WS / message_queue).
pub fn parse_sent_message(bytes: &[u8]) -> Result<Vec<RecipientFanout>, CryptoError> {
    let parsed = signal::SealedSenderV2SentMessage::parse(bytes)?;
    let mut out = Vec::with_capacity(parsed.recipients.len());
    for (service_id, recipient) in &parsed.recipients {
        let parts = parsed.received_message_parts_for_recipient(recipient);
        let received_message: Vec<u8> = parts.as_ref().concat();
        let devices: Vec<(u32, u16)> = recipient
            .devices
            .iter()
            .map(|(dev, reg)| (u32::from(*dev), *reg))
            .collect();
        out.push(RecipientFanout {
            service_id_fixed_width: service_id.service_id_fixed_width_binary(),
            devices,
            received_message,
        });
    }
    Ok(out)
}

/// Decrypted USMC fields, returned to the caller for sender-cert validation
/// and inner-payload (SenderKey ciphertext) decryption.
#[derive(Debug, Clone)]
pub struct DecryptedEnvelope {
    /// Serialized `SenderCertificate` — pass to `sender_cert::validate_sender_cert`.
    pub sender_cert_bytes: Vec<u8>,
    /// Inner ciphertext (the bytes originally returned by `group_encrypt`).
    pub contents: Vec<u8>,
    /// `group_id` set by the sender (if any).
    pub group_id: Option<Vec<u8>>,
}

/// Recipient-side: decrypt a SSv2 ReceivedMessage envelope down to its USMC.
/// Does **not** validate the embedded sender certificate against any trust
/// root — the caller must call [`crate::sender_cert::validate_sender_cert`]
/// with the bytes from `sender_cert_bytes` before trusting the contents.
pub async fn decrypt_envelope_to_usmc<S: Store>(
    store: &mut S,
    envelope: &[u8],
) -> Result<DecryptedEnvelope, CryptoError> {
    let identity_store = store.clone();
    let usmc = signal::sealed_sender_decrypt_to_usmc(envelope, &identity_store).await?;
    if usmc.msg_type()? != CiphertextMessageType::SenderKey {
        return Err(CryptoError::InvalidCiphertext);
    }
    Ok(DecryptedEnvelope {
        sender_cert_bytes: usmc.sender()?.serialized()?.to_vec(),
        contents: usmc.contents()?.to_vec(),
        group_id: usmc.group_id()?.map(|g| g.to_vec()),
    })
}
