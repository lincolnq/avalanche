//! DM send/receive plumbing. Owns the methods on `AppCoreInner` that wrap
//! libsignal's per-device encrypt/decrypt and the fan-out to a recipient's
//! device list, plus `process_decrypted` — the WebSocket-path equivalent of
//! `receive_messages`'s envelope-dispatch arm.

use crypto::session::{self, DeviceAddress, EncryptedMessage, MessageKind};
use net::types::OutboundMessage;
use prost::Message as _;
use types::{AccountId, DeviceId, Timestamp};

use crate::groups;
use crate::profile;
use crate::proto::{
    self, content_message::Body, delete_message, receipt_message, ContentMessage, DeleteMessage,
    EditMessage, ReactionMessage, ReceiptMessage,
};
use crate::{
    AppCore, AppCoreInner, AppError, DecryptedMessage, DeliveryStatusUpdate, IncomingEvent,
    MessageTarget,
};

/// Outcome of a server name-fetch attempt, used as the throttle key
/// (docs/52). The integer codes are persisted in `profile_fetch_state.outcome`
/// — keep them stable. Windows mirror Signal's `ProfileFetcher` LRU
/// (`docs/signal-research/profile-key-transmission.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FetchOutcome {
    Success = 0,
    Network = 1,
    NotAuthorized = 2,
    NotFound = 3,
    RateLimited = 4,
    Other = 5,
}

impl FetchOutcome {
    fn code(self) -> i64 {
        self as i64
    }

    fn from_code(c: i64) -> Self {
        match c {
            0 => Self::Success,
            1 => Self::Network,
            2 => Self::NotAuthorized,
            3 => Self::NotFound,
            4 => Self::RateLimited,
            _ => Self::Other,
        }
    }

    /// Minimum time before another attempt is allowed.
    fn skip_window_ms(self) -> i64 {
        match self {
            Self::Success => 5 * 60 * 1000,
            Self::Network => 60 * 1000,
            Self::NotAuthorized => 30 * 60 * 1000,
            Self::NotFound => 6 * 60 * 60 * 1000,
            Self::RateLimited => 5 * 60 * 1000,
            Self::Other => 30 * 60 * 1000,
        }
    }
}

/// Map a net error onto a throttle outcome so failures get an appropriate
/// negative-cache window (404 → 6h, 401/403 → 30m, transport → 1m, etc.).
pub(crate) fn classify_net_error(e: &net::error::NetError) -> FetchOutcome {
    use net::error::NetError;
    match e {
        NetError::Server(401 | 403, _) => FetchOutcome::NotAuthorized,
        NetError::Server(404, _) => FetchOutcome::NotFound,
        NetError::Server(429, _) => FetchOutcome::RateLimited,
        NetError::Server(_, _) => FetchOutcome::Other,
        NetError::Http(_) | NetError::WebSocket(_) => FetchOutcome::Network,
        _ => FetchOutcome::Other,
    }
}

/// Ensure a usable Double Ratchet session exists for `(recipient_did, device_id)`.
///
/// If there is no session (or `force_refresh` is set), fetch that device's
/// prekey bundle and run X3DH. `force_refresh` discards any existing session
/// first — used after a stale-device signal (DM path) or when the peer's
/// registration id has changed (group path).
pub(crate) async fn ensure_session(
    store: &mut store::DeviceStore,
    client: &net::Client,
    local: &DeviceAddress,
    recipient_did: &str,
    device_id: u32,
    force_refresh: bool,
) -> Result<(), AppError> {
    let recipient_sid = crypto::groups::did_to_service_id_string(recipient_did);
    let recipient_addr =
        DeviceAddress::new(AccountId::new(&recipient_sid), DeviceId::new(device_id));

    if !force_refresh {
        use libsignal_protocol::SessionStore;
        let protocol_addr = libsignal_protocol::ProtocolAddress::new(
            recipient_sid.clone(),
            libsignal_protocol::DeviceId::try_from(device_id).unwrap(),
        );
        if store
            .load_session(&protocol_addr)
            .await
            .map_err(|e| AppError::Crypto(crypto::CryptoError::Signal(e)))?
            .is_some()
        {
            return Ok(());
        }
    }

    let bundle = client
        .fetch_prekey_bundle(recipient_did, device_id as i32)
        .await?;
    let recipient_bundle = crypto::RecipientKeyBundle {
        identity_key: bundle.identity_key.clone(),
        registration_id: bundle.registration_id as u32,
        device_id,
        signed_prekey: crypto::SignedPreKey {
            id: bundle.signed_prekey_id as u32,
            public_key: bundle.signed_prekey_public,
            signature: bundle.signed_prekey_signature,
        },
        one_time_prekey: bundle.one_time_prekey.map(|(id, pk)| crypto::OneTimePreKey {
            id: id as u32,
            public_key: pk,
        }),
        kyber_prekey: crypto::prekeys::KyberPreKey {
            id: bundle.kyber_prekey_id as u32,
            public_key: bundle.kyber_prekey_public,
            signature: bundle.kyber_prekey_signature,
        },
    };
    session::initiate_session(store, local, &recipient_addr, &recipient_bundle).await?;
    Ok(())
}

/// Reconcile the sender's sessions with every active device of `recipient_did`
/// ahead of a group sealed-sender fan-out, returning the device-id list to
/// address.
///
/// For each device the server currently reports we:
/// - establish a session if we don't have one (a member we never DM'd, or one
///   added after us whose join-time SKDM never landed), and
/// - re-run X3DH if our session's registration id is stale relative to the
///   server's (the peer re-registered the device) — the case the sealed-sender
///   send endpoint can't signal back, so we detect it here.
///
/// This is what lets `encrypt_group_envelope` find a session for every
/// destination instead of failing with `NoSession`.
pub(crate) async fn ensure_group_recipient_sessions(
    store: &mut store::DeviceStore,
    client: &net::Client,
    sender_did: &str,
    sender_device_id: u32,
    recipient_did: &str,
) -> Result<Vec<u32>, AppError> {
    let local = DeviceAddress::new(
        AccountId::new(crypto::groups::did_to_service_id_string(sender_did)),
        DeviceId::new(sender_device_id),
    );
    let devices = client.fetch_device_registrations(recipient_did).await?;
    if devices.is_empty() {
        return Err(AppError::Protocol(format!(
            "no active devices for recipient {recipient_did}"
        )));
    }

    let recipient_sid = crypto::groups::did_to_service_id_string(recipient_did);
    let mut device_ids = Vec::with_capacity(devices.len());
    for d in devices {
        let device_id = d.device_id as u32;
        let recipient_addr =
            DeviceAddress::new(AccountId::new(&recipient_sid), DeviceId::new(device_id));
        let current_reg = session::remote_registration_id(store, &recipient_addr).await?;
        let force = matches!(current_reg, Some(reg) if reg != d.registration_id as u32);
        ensure_session(store, client, &local, recipient_did, device_id, force).await?;
        device_ids.push(device_id);
    }
    Ok(device_ids)
}

/// Message-request gate decision for an inbound DM sender (docs/12 §1).
pub(crate) struct SenderGate {
    pub is_curated: bool,
    pub is_blocked: bool,
    pub is_bot: bool,
}

impl SenderGate {
    /// True when the message delivers as a normal (non-request) DM: the sender
    /// is curated, or is a homeserver-known bot (docs/12 §"When is a sender
    /// known"). Everyone else is gated behind the message-request UI.
    pub(crate) fn passes(&self) -> bool {
        self.is_curated || self.is_bot
    }
}

impl AppCoreInner {
    /// Process an inbound `profile_key` from a ContentMessage. If non-empty
    /// and different from any cached key, fetch the sender's encrypted blob,
    /// decrypt, and update the contact_profiles cache. Errors are swallowed
    /// — profile fetches are best-effort and must never block message
    /// delivery.
    pub(crate) async fn handle_inbound_profile_key(&self, sender_did: &str, profile_key: &[u8]) {
        if profile_key.len() != profile::PROFILE_KEY_LEN {
            return;
        }

        let needs_fetch = match self.store.load_contact_profile_key(sender_did).await {
            Ok(Some(cached)) => cached != profile_key,
            Ok(None) => true,
            Err(_) => return,
        };
        if !needs_fetch {
            return;
        }

        let blob = match self.client.get_profile(sender_did).await {
            Ok(Some(b)) => b,
            Ok(None) | Err(_) => return,
        };

        let mut key = [0u8; profile::PROFILE_KEY_LEN];
        key.copy_from_slice(profile_key);
        let plaintext = match profile::decrypt_profile(&blob, &key) {
            Ok(p) => p,
            Err(_) => return,
        };

        let _ = self
            .store
            .upsert_contact_profile(&store::profiles::ContactProfile {
                did: sender_did.to_string(),
                display_name: plaintext.display_name,
                profile_key: profile_key.to_vec(),
                fetched_at: Timestamp::now(),
            })
            .await;
    }

    /// Whether a name fetch for `did` is allowed right now, per the persisted
    /// per-outcome throttle (docs/52 §"Client-side rate limiting"). `None`
    /// (never attempted) → fetch. Otherwise honor the skip window for the last
    /// outcome. Errors reading the throttle fail open (allow the fetch).
    pub(crate) async fn should_fetch_name(&self, did: &str) -> bool {
        match self.store.load_fetch_state(did).await {
            Ok(Some((last_attempt, code))) => {
                let window = FetchOutcome::from_code(code).skip_window_ms();
                Timestamp::now().as_millis() - last_attempt.as_millis() >= window
            }
            Ok(None) => true,
            Err(_) => true,
        }
    }

    /// Record the outcome of a name fetch for `did` so the throttle (and the
    /// negative cache for failures) survives launches. Best-effort.
    pub(crate) async fn record_fetch(&self, did: &str, outcome: FetchOutcome) {
        let _ = self
            .store
            .record_fetch_attempt(did, outcome.code(), Timestamp::now())
            .await;
    }

    /// Load the user's own profile key as bytes, or empty if not yet set.
    /// Empty bytes in the proto field signal "not sharing" — recipients ignore
    /// a zero-length `profile_key` field.
    pub(crate) async fn own_profile_key(&self) -> Vec<u8> {
        self.store
            .load_own_profile()
            .await
            .ok()
            .flatten()
            .map(|p| p.profile_key)
            .unwrap_or_default()
    }

    /// Re-build and upload the user's recovery blob from current local
    /// state, using the cached PRF-derived blob key. Idempotent and
    /// best-effort: silently no-ops if no blob_key is cached (opt-out
    /// account) or if there's no registered server yet.
    ///
    /// Called by every site that changes blob-relevant state — primarily
    /// group joins (founder + invitee) so a fresh recovery preserves
    /// membership in groups the user joined post-signup.
    pub(crate) async fn refresh_recovery_blob_best_effort(&self) {
        if let Err(e) = self.refresh_recovery_blob_inner().await {
            tracing::warn!("[recovery] auto-upload failed: {e}");
        }
    }

    async fn refresh_recovery_blob_inner(&self) -> Result<(), AppError> {
        let key = match self.store.load_recovery_blob_key().await? {
            Some(k) => k,
            None => return Ok(()),
        };
        let identity = self
            .store
            .load_identity()
            .await?
            .ok_or(AppError::NoAccount)?;
        let own_profile = self.store.load_own_profile().await?;
        let groups = crate::recovery::collect_group_blob_entries(&self.store).await?;
        let storage_key = self.store.load_storage_key().await?;
        let server_url = self.client.server_url().to_string();
        let plaintext = crate::recovery::build_recovery_blob(
            &identity.serialize(),
            &[server_url],
            own_profile
                .as_ref()
                .map(|p| p.profile_key.as_slice())
                .unwrap_or(&[]),
            own_profile
                .as_ref()
                .map(|p| p.display_name.as_str())
                .unwrap_or(""),
            &groups,
            storage_key.as_ref().map(|k| k.as_slice()).unwrap_or(&[]),
        );
        let blob = crate::recovery::encrypt_recovery_blob(&plaintext, &key)?;
        self.client.update_recovery_blob(&blob).await?;
        Ok(())
    }

    /// Decrypt a `GroupDeliverRequest` arriving over the WebSocket, surface
    /// it as `IncomingEvent::Message` with `group_id` populated, and ack.
    /// Mirrors the per-message handling in `fetch_group_messages` so the
    /// WS push path and the explicit poll path produce identical events.
    pub(crate) async fn process_inbound_group_delivery(
        &mut self,
        delivery: &net::ws::InboundGroupDelivery,
    ) -> Result<DecryptedMessage, AppError> {
        let group_id_b64 = groups::b64(&delivery.group_id);
        let group_row = self
            .store
            .load_group(&group_id_b64)
            .await?
            .ok_or_else(|| {
                AppError::Protocol(format!(
                    "group {group_id_b64} not in local store; cannot process push"
                ))
            })?;
        let trust_root = groups::load_sender_cert_trust_root(
            &self.store,
            &self.client,
            &group_row.hosting_server_url,
        )
        .await?;
        let env =
            crypto::sealed_sender::decrypt_envelope_to_usmc(&mut self.store, &delivery.ciphertext)
                .await
                .map_err(|e| AppError::Protocol(format!("sealed_sender decrypt: {e}")))?;
        let now_ms = Timestamp::now().as_millis() as u64;
        let info = crypto::sender_cert::validate_sender_cert(
            &env.sender_cert_bytes,
            &trust_root,
            now_ms,
        )
        .map_err(|e| AppError::Protocol(format!("sender_cert validate: {e}")))?;
        let plaintext = groups::decrypt_group_content(
            &mut self.store,
            &info.sender_did,
            info.sender_device_id,
            &env.contents,
        )
        .await?;
        Ok(DecryptedMessage {
            server_id: delivery.message_id,
            sender_did: info.sender_did,
            sender_device_id: info.sender_device_id,
            group_id: Some(group_id_b64),
            plaintext,
            sent_at_ms: None,
        })
    }

    /// Finalize joining a group after the master key has been persisted:
    /// fetch+cache the current group state, submit the `accept` action,
    /// then seed our local Sender Key state and DM the resulting SKDM to
    /// every existing member so they can decrypt our future group
    /// messages. Used by both the FFI `accept_invite` and the auto-accept
    /// path on incoming `GroupContext`.
    pub(crate) async fn complete_join_group(
        &mut self,
        ws: Option<&net::ws::WsConnection>,
        hosting_server_url: &str,
        group_id_b64: &str,
    ) -> Result<(), AppError> {
        let did = self.did.clone();
        let device_id = self.device_id;

        // `accept_invite` works against the cached group state, which the
        // master-key-only persistence path doesn't populate.
        groups::fetch_group_state(
            &self.store,
            &self.client,
            hosting_server_url,
            &did,
            group_id_b64,
        )
        .await?;
        groups::accept_invite(&self.store, &self.client, &did, group_id_b64).await?;

        // `accept_invite` registered a fresh `group_push_pseudonym`. Tell
        // the server to push future fan-outs for it on this WS so we
        // don't have to poll `fetch_group_messages` to see new messages.
        // Best-effort: a missing WS or send error is non-fatal — the
        // reconnect-time subscription handles steady-state.
        if let (Some(ws), Some(pseudonym)) = (
            ws,
            self.store
                .load_group(group_id_b64)
                .await
                .ok()
                .flatten()
                .and_then(|g| g.group_push_pseudonym),
        ) {
            if let Err(e) = ws.subscribe_group_pseudonyms(vec![pseudonym]) {
                tracing::warn!(
                    "[groups] subscribe to new group pseudonym for {group_id_b64} failed: {e}"
                );
            }
        }

        let mk = groups::master_key_for(&self.store, group_id_b64).await?;
        let skdm = groups::seed_own_sender_key(&mut self.store, &did, device_id, &mk).await?;
        let group_id_bytes = groups::b64d(group_id_b64)?;
        let recipients = groups::other_member_dids(&self.store, group_id_b64, &did).await?;
        for rdid in recipients {
            let skdm_msg = ContentMessage {
                body: Some(Body::SenderKeyDistribution(proto::SenderKeyDistribution {
                    group_id: group_id_bytes.clone(),
                    distribution_id: groups::distribution_id_for(&mk).as_bytes().to_vec(),
                    skdm: skdm.clone(),
                })),
                timestamp_ms: Timestamp::now().as_millis() as u64,
                profile_key: Vec::new(),
            };
            if let Err(e) = self
                .send_dm(ws, &rdid, &skdm_msg.encode_to_vec(), None)
                .await
            {
                tracing::warn!("[groups] SKDM DM to {rdid} failed: {e}");
            }
        }

        // Sync the server-side recovery blob so a future recovery sees
        // this new group. Best-effort.
        self.refresh_recovery_blob_best_effort().await;

        Ok(())
    }

    /// Send raw bytes (already enveloped) as an encrypted DM.
    /// Fan-out send: fetches the recipient's active device list and encrypts a
    /// copy of `plaintext` for each device, sending them as one batch.
    ///
    /// If the server returns 409 (stale device), automatically re-establishes
    /// the session for affected devices and retries once.
    pub(crate) async fn send_dm(
        &mut self,
        ws: Option<&net::ws::WsConnection>,
        recipient_did: &str,
        plaintext: &[u8],
        expiry_secs: Option<i64>,
    ) -> Result<(), AppError> {
        let device_ids = self.client.fetch_devices(recipient_did).await?;
        if device_ids.is_empty() {
            return Err(AppError::Protocol(format!(
                "no active devices for recipient {recipient_did}"
            )));
        }

        let envelopes = self
            .build_envelopes(recipient_did, &device_ids, plaintext, expiry_secs, &[])
            .await?;

        // Prefer the WebSocket when open — saves a TCP handshake per send.
        // Fall back to HTTP on no connection or WS-side failure (the server's
        // HTTP route is the same enqueue path either way).
        if let Some(ws) = ws {
            let proto_msgs: Vec<net::proto::OutboundMessage> = envelopes
                .iter()
                .map(|e| net::proto::OutboundMessage {
                    recipient_did: e.recipient_did.clone(),
                    recipient_device_id: e.recipient_device_id,
                    destination_registration_id: e.destination_registration_id,
                    ciphertext: e.ciphertext.clone(),
                    message_kind: e.message_kind as i32,
                    expiry_secs: e.expiry_secs,
                })
                .collect();
            match ws.send_messages(proto_msgs).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::debug!("[ws] send failed, falling back to HTTP: {e}");
                }
            }
        }
        match self.client.send_messages(&envelopes).await {
            Ok(_) => Ok(()),
            Err(net::error::NetError::StaleDevice { stale_devices }) => {
                let stale_ids: Vec<i32> = stale_devices.iter().map(|s| s.device_id).collect();
                let envelopes = self
                    .build_envelopes(recipient_did, &device_ids, plaintext, expiry_secs, &stale_ids)
                    .await?;
                self.client.send_messages(&envelopes).await?;
                Ok(())
            }
            Err(e) => Err(AppError::Net(e)),
        }
    }

    /// Encrypt `plaintext` for each device in `device_ids`.
    /// Devices in `force_refresh_ids` will have their sessions forcibly re-established.
    async fn build_envelopes(
        &mut self,
        recipient_did: &str,
        device_ids: &[i32],
        plaintext: &[u8],
        expiry_secs: Option<i64>,
        force_refresh_ids: &[i32],
    ) -> Result<Vec<OutboundMessage>, AppError> {
        let mut envelopes = Vec::with_capacity(device_ids.len());
        for &device_id in device_ids {
            let force = force_refresh_ids.contains(&device_id);
            envelopes.push(
                self.encrypt_for_device(recipient_did, device_id as u32, plaintext, expiry_secs, force)
                    .await?,
            );
        }
        Ok(envelopes)
    }

    /// Per-device encryption helper: establishes a Double Ratchet session if
    /// needed (fetching that device's prekey bundle), encrypts the plaintext,
    /// and returns the `OutboundMessage` envelope ready for `send_messages`.
    ///
    /// If `force_refresh` is true, the existing session (if any) is discarded
    /// and a fresh prekey bundle is fetched — used after a 409 stale-device response.
    async fn encrypt_for_device(
        &mut self,
        recipient_did: &str,
        recipient_device_id: u32,
        plaintext: &[u8],
        expiry_secs: Option<i64>,
        force_refresh: bool,
    ) -> Result<OutboundMessage, AppError> {
        let recipient_sid = crypto::groups::did_to_service_id_string(recipient_did);
        let recipient_addr = DeviceAddress::new(
            AccountId::new(&recipient_sid),
            DeviceId::new(recipient_device_id),
        );

        ensure_session(
            &mut self.store,
            &self.client,
            &self.local_address,
            recipient_did,
            recipient_device_id,
            force_refresh,
        )
        .await?;

        let encrypted = session::encrypt(
            &mut self.store,
            &self.local_address,
            &recipient_addr,
            plaintext,
        )
        .await?;

        let dest_reg_id = session::remote_registration_id(&mut self.store, &recipient_addr)
            .await?
            .ok_or_else(|| AppError::Protocol("no session after encrypt".into()))?
            as i32;

        Ok(OutboundMessage {
            recipient_did: recipient_did.to_string(),
            recipient_device_id: recipient_device_id as i32,
            destination_registration_id: dest_reg_id,
            ciphertext: encrypted.ciphertext,
            message_kind: match encrypted.kind {
                MessageKind::PreKey => 0,
                MessageKind::Whisper => 1,
            },
            expiry_secs,
        })
    }

    /// Apply an inbound edit to the DM with `sender` and return the event to
    /// surface, or `None` if nothing changed. Authorship is implicit: an edit
    /// only targets the sender's own message, so we key the store update on
    /// `(conv, sender, target_sent_at)` — it can never touch another author's
    /// message (docs/36). `op_sent_at_ms` is the edit's LWW clock.
    pub(crate) async fn apply_inbound_edit(
        &self,
        conv_id: &str,
        sender: &str,
        edit: EditMessage,
        op_sent_at_ms: u64,
    ) -> Option<IncomingEvent> {
        let new_body = edit.replacement.map(|t| t.body).unwrap_or_default();
        let target_sent_at = edit.target_sent_at as i64;
        let applied = self
            .store
            .apply_edit(
                conv_id,
                sender,
                Timestamp(target_sent_at),
                &new_body,
                Timestamp(op_sent_at_ms as i64),
                true,
            )
            .await
            .unwrap_or(false);
        applied.then_some(IncomingEvent::MessageEdited {
            conversation_id: conv_id.to_string(),
            author_did: sender.to_string(),
            sent_at_ms: target_sent_at,
            new_body,
            edited_at_ms: op_sent_at_ms as i64,
        })
    }

    /// Apply an inbound delete to the DM with `sender`. FOR_EVERYONE tombstones
    /// the target (authorship-gated: the op's authenticated sender must equal
    /// `target_author`). FOR_ME from a peer only affects the peer's own view,
    /// so it's ignored here (docs/36).
    pub(crate) async fn apply_inbound_delete(
        &self,
        conv_id: &str,
        sender: &str,
        del: DeleteMessage,
        op_sent_at_ms: u64,
    ) -> Option<IncomingEvent> {
        if del.scope != delete_message::Scope::ForEveryone as i32 {
            return None;
        }
        // The load-bearing security check: a FOR_EVERYONE delete is honored
        // only if its authenticated sender authored the target message.
        if del.target_author != sender {
            return None;
        }
        let target_sent_at = del.target_sent_at as i64;
        self.store
            .tombstone_message(
                conv_id,
                &del.target_author,
                Timestamp(target_sent_at),
                Timestamp(op_sent_at_ms as i64),
            )
            .await
            .ok()?;
        Some(IncomingEvent::MessageDeleted {
            conversation_id: conv_id.to_string(),
            author_did: del.target_author,
            sent_at_ms: target_sent_at,
        })
    }

    /// Apply an inbound reaction to the DM with `sender` (the reactor). The
    /// target message is `(target_author, target_sent_at)`; stored keyed on the
    /// wire identity so it converges even if it arrives before the target
    /// (docs/33).
    pub(crate) async fn apply_inbound_reaction(
        &self,
        conv_id: &str,
        sender: &str,
        r: ReactionMessage,
        op_sent_at_ms: u64,
    ) -> Option<IncomingEvent> {
        let target_sent_at = r.target_sent_at as i64;
        if r.remove {
            self.store
                .remove_reaction(conv_id, &r.target_author, Timestamp(target_sent_at), sender)
                .await
                .ok()?;
        } else {
            self.store
                .upsert_reaction(&store::messages::ReactionRow {
                    conversation_id: conv_id.to_string(),
                    target_author: r.target_author.clone(),
                    target_sent_at: Timestamp(target_sent_at),
                    reactor_did: sender.to_string(),
                    emoji: r.emoji.clone(),
                    reacted_at: Timestamp(op_sent_at_ms as i64),
                })
                .await
                .ok()?;
        }
        Some(IncomingEvent::ReactionUpdated {
            conversation_id: conv_id.to_string(),
            target_author: r.target_author,
            target_sent_at_ms: target_sent_at,
            reactor_did: sender.to_string(),
            emoji: r.emoji,
            removed: r.remove,
        })
    }

    /// Build a `ContentMessage` envelope around `body` (with our timestamp and
    /// profile key) and fan it out to the group via the sealed-sender path.
    /// This is the group analogue of the DM envelope wrapping in `send_dm`:
    /// all group content now rides a `ContentMessage`, so receipts/reactions/
    /// edits/deletes work in groups exactly as in DMs.
    pub(crate) async fn send_group_content(
        &mut self,
        group_id: &str,
        body: Body,
        sent_at_ms: u64,
    ) -> Result<(), AppError> {
        let did = self.did.clone();
        let device_id = self.device_id;
        let server_url = self.client.server_url().to_string();
        let profile_key = self.own_profile_key().await;
        let msg = ContentMessage {
            body: Some(body),
            timestamp_ms: sent_at_ms,
            profile_key,
        };
        let bytes = msg.encode_to_vec();
        let AppCoreInner {
            ref mut store,
            ref client,
            ..
        } = *self;
        groups::send_group_message(store, client, &server_url, &did, device_id, group_id, &bytes)
            .await?;
        Ok(())
    }

    /// Send a `ContentMessage` body to a conversation target — the single place
    /// the DM and group transports fork. DMs wrap the body in an envelope and
    /// fan out per-device (Double Ratchet); groups reuse `send_group_content`
    /// (sealed-sender + sender keys). Used by the unified `send_reaction` /
    /// `send_edit` / `send_delete` FFI methods.
    pub(crate) async fn send_to_target(
        &mut self,
        ws: Option<&net::ws::WsConnection>,
        target: &MessageTarget,
        body: Body,
        sent_at_ms: u64,
    ) -> Result<(), AppError> {
        match target {
            MessageTarget::Dm { recipient_did } => {
                let profile_key = self.own_profile_key().await;
                let msg = ContentMessage {
                    body: Some(body),
                    timestamp_ms: sent_at_ms,
                    profile_key,
                };
                self.send_dm(ws, recipient_did, &msg.encode_to_vec(), None).await
            }
            MessageTarget::Group { group_id } => {
                self.send_group_content(group_id, body, sent_at_ms).await
            }
        }
    }

    /// Evaluate the message-request gate for an inbound DM sender (docs/12 §1,
    /// docs/52 §"What is_curated drives"). A sender delivers as a normal DM iff
    /// curated or a homeserver-known bot; an un-curated human is a *request*; a
    /// blocked DID is dropped after decryption.
    pub(crate) async fn sender_gate(&self, did: &str) -> SenderGate {
        let contact = self.store.load_contact(did).await.ok().flatten();
        let is_bot = self
            .store
            .load_account_info(did)
            .await
            .ok()
            .flatten()
            .map(|a| a.is_bot)
            .unwrap_or(false);
        SenderGate {
            is_curated: contact.as_ref().map(|c| c.is_curated).unwrap_or(false),
            is_blocked: contact.as_ref().map(|c| c.is_blocked).unwrap_or(false),
            is_bot,
        }
    }

    /// Refuse a user-initiated outbound DM to a blocked DID (docs/12 §2).
    /// Plumbing sends (delivery receipts, SKDM fan-out) deliberately don't call
    /// this, so blocking a group co-member never breaks group crypto.
    pub(crate) async fn ensure_not_blocked(&self, did: &str) -> Result<(), AppError> {
        let blocked = self
            .store
            .load_contact(did)
            .await?
            .map(|c| c.is_blocked)
            .unwrap_or(false);
        if blocked {
            return Err(AppError::Blocked(did.to_string()));
        }
        Ok(())
    }

    pub(crate) async fn receive_messages(
        &mut self,
        ws: Option<&net::ws::WsConnection>,
    ) -> Result<Vec<DecryptedMessage>, AppError> {
        let inbound = self.client.fetch_messages().await?;
        let mut decrypted = Vec::with_capacity(inbound.len());

        for msg in &inbound {
            let raw = self.decrypt_inbound(msg).await?;
            // Parse envelope: unwrap content, skip receipts (handled internally).
            match ContentMessage::decode(raw.plaintext.as_slice()) {
                Ok(content) => {
                    if !content.profile_key.is_empty() {
                        // Note that this will block on network if we have not already downloaded the contact's profile blob
                        self.handle_inbound_profile_key(&raw.sender_did, &content.profile_key).await;
                    }
                    match content.body {
                        Some(Body::Receipt(receipt)) => {
                            let status: u8 = if receipt.r#type == receipt_message::Type::Read as i32 {
                                3
                            } else {
                                2
                            };
                            let timestamps: Vec<i64> =
                                receipt.timestamps.into_iter().map(|t| t as i64).collect();
                            if !timestamps.is_empty() {
                                let conv_id = format!("dm-{}-{}", self.did, raw.sender_did);
                                let _ = self
                                    .store
                                    .update_delivery_status(&conv_id, &timestamps, status)
                                    .await;
                            }
                        }
                        Some(Body::Text(text)) => {
                            let body = text.body;
                            let sent_at = if content.timestamp_ms > 0 {
                                Some(content.timestamp_ms as i64)
                            } else {
                                None
                            };
                            // Message-request gate (DM only — /v1/messages
                            // carries DMs). A blocked sender's message was
                            // decrypted to advance the ratchet, then dropped:
                            // no event, no delivery receipt (docs/12 §2).
                            let gate = self.sender_gate(&raw.sender_did).await;
                            if gate.is_blocked {
                                continue;
                            }
                            // Recency bump (non-curating). An un-curated human
                            // is a pending request; the flag lets Delete dismiss
                            // it without curating (docs/52).
                            let _ = self
                                .store
                                .touch_contact(&raw.sender_did, false, Timestamp::now())
                                .await;
                            if !gate.passes() {
                                let _ = self
                                    .store
                                    .set_pending_request(&raw.sender_did, true)
                                    .await;
                            }
                            // Auto-send delivery receipt — allowed even for an
                            // un-accepted request (docs/12 §1), DM only.
                            if let Some(ts) = sent_at {
                                let profile_key = self.own_profile_key().await;
                                let delivery = ContentMessage {
                                    body: Some(Body::Receipt(ReceiptMessage {
                                        r#type: receipt_message::Type::Delivery as i32,
                                        timestamps: vec![ts as u64],
                                    })),
                                    timestamp_ms: 0,
                                    profile_key,
                                };
                                let _ = self
                                    .send_dm(ws, &raw.sender_did, &delivery.encode_to_vec(), None)
                                    .await;
                            }
                            decrypted.push(DecryptedMessage {
                                plaintext: body.into_bytes(),
                                sent_at_ms: sent_at,
                                ..raw
                            });
                        }
                        Some(Body::GroupContext(ctx)) => {
                            if let Err(e) = groups::store_inbound_group_context(
                                &self.store,
                                &ctx.group_master_key,
                                &ctx.hosting_server_url,
                            )
                            .await
                            {
                                tracing::warn!(
                                    "[groups] failed to store inbound GroupContext: {e}"
                                );
                            }
                            decrypted.push(raw);
                        }
                        Some(Body::SenderKeyDistribution(skdm_msg)) => {
                            // Install the sender's group key locally so
                            // future `GroupMessage`s from them decrypt.
                            // No app-level event — SKDM is plumbing.
                            if let Err(e) = groups::process_inbound_skdm(
                                &mut self.store,
                                &raw.sender_did,
                                raw.sender_device_id,
                                &skdm_msg.skdm,
                            )
                            .await
                            {
                                tracing::warn!(
                                    "[groups] failed to process inbound SKDM: {e}"
                                );
                            }
                        }
                        Some(Body::GroupMessage(gm)) => {
                            match groups::decrypt_group_content(
                                &mut self.store,
                                &raw.sender_did,
                                raw.sender_device_id,
                                &gm.ciphertext,
                            )
                            .await
                            {
                                Ok(plaintext) => {
                                    decrypted.push(DecryptedMessage {
                                        plaintext,
                                        group_id: Some(groups::b64(&gm.group_id)),
                                        ..raw
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "[groups] failed to decrypt GroupMessage: {e}"
                                    );
                                }
                            }
                        }
                        Some(Body::TimerChange(timer)) => {
                            // Silent control message — update local expiry setting,
                            // no visible chat event surfaced.
                            let expiry = if timer.expiry_secs > 0 {
                                Some(timer.expiry_secs)
                            } else {
                                None
                            };
                            let _ = self
                                .store
                                .save_conversation_expiry(&raw.sender_did, expiry)
                                .await;
                        }
                        Some(Body::Edit(edit)) => {
                            // Apply to the store; no DecryptedMessage to surface
                            // on the polling path (the WS path emits an event).
                            // /v1/messages carries DMs, so the conv is a DM.
                            let conv_id = format!("dm-{}-{}", self.did, raw.sender_did);
                            let _ = self
                                .apply_inbound_edit(&conv_id, &raw.sender_did, edit, content.timestamp_ms)
                                .await;
                        }
                        Some(Body::Delete(del)) => {
                            let conv_id = format!("dm-{}-{}", self.did, raw.sender_did);
                            let _ = self
                                .apply_inbound_delete(&conv_id, &raw.sender_did, del, content.timestamp_ms)
                                .await;
                        }
                        Some(Body::Reaction(reaction)) => {
                            let conv_id = format!("dm-{}-{}", self.did, raw.sender_did);
                            let _ = self
                                .apply_inbound_reaction(
                                    &conv_id,
                                    &raw.sender_did,
                                    reaction,
                                    content.timestamp_ms,
                                )
                                .await;
                        }
                        None => {
                            // ContentMessage with no body — backward compat.
                            decrypted.push(raw);
                        }
                    }
                }
                Err(_) => {
                    // Not valid protobuf — backward compat: raw plaintext.
                    decrypted.push(raw);
                }
            }
        }

        if !inbound.is_empty() {
            let ids: Vec<i64> = inbound.iter().map(|m| m.id).collect();
            self.client.ack_messages(&ids).await?;
        }

        Ok(decrypted)
    }

    pub(crate) async fn decrypt_inbound(
        &mut self,
        msg: &net::types::InboundMessage,
    ) -> Result<DecryptedMessage, AppError> {
        let sender_did = msg
            .sender_did
            .as_deref()
            .ok_or_else(|| AppError::Protocol("message missing sender_did".into()))?;
        let sender_device_id = msg
            .sender_device_id
            .ok_or_else(|| AppError::Protocol("message missing sender_device_id".into()))?
            as u32;

        let sender_sid = crypto::groups::did_to_service_id_string(sender_did);
        let sender_addr = DeviceAddress::new(
            AccountId::new(&sender_sid),
            DeviceId::new(sender_device_id),
        );

        let encrypted = EncryptedMessage {
            ciphertext: msg.ciphertext.clone(),
            kind: if msg.message_kind == 0 {
                MessageKind::PreKey
            } else {
                MessageKind::Whisper
            },
        };

        let plaintext = session::decrypt(
            &mut self.store,
            &self.local_address,
            &sender_addr,
            &encrypted,
        )
        .await?;

        Ok(DecryptedMessage {
            server_id: msg.id,
            sender_did: sender_did.to_string(),
            sender_device_id,
            plaintext,
            sent_at_ms: None,
            group_id: None,
        })
    }
}

/// Decode a `DecryptedMessage`'s content envelope and emit messages /
/// receipt-updates onto the event channel. Sends auto-delivery receipts.
/// WebSocket equivalent of the envelope-dispatch arm inside
/// `AppCoreInner::receive_messages`.
pub(crate) async fn process_decrypted(core: &AppCore, decrypted: DecryptedMessage) {
    let msg = match ContentMessage::decode(decrypted.plaintext.as_slice()) {
        Ok(m) => m,
        Err(_) => {
            // Non-protobuf payload — emit as raw text for backward compat.
            let _ = core.event_tx.send(IncomingEvent::Message { msg: decrypted });
            return;
        }
    };

    // Process inbound profile_key (any variant may carry one).
    if !msg.profile_key.is_empty() {
        let inner = core.inner.lock().await;
        inner
            .handle_inbound_profile_key(&decrypted.sender_did, &msg.profile_key)
            .await;
    }

    // The conversation this content belongs to: the group thread for group
    // deliveries, otherwise the DM with the sender. Edit/delete/reaction and
    // receipt application all key on this.
    let conv_id = match decrypted.group_id.as_deref() {
        Some(g) => format!("group-{g}"),
        None => format!("dm-{}-{}", core.inner.lock().await.did, decrypted.sender_did),
    };

    match msg.body {
        Some(Body::Receipt(receipt)) => {
            // DELIVERY (0) → status 2, READ (1) → status 3.
            let status: u8 = if receipt.r#type == receipt_message::Type::Read as i32 {
                3
            } else {
                2
            };
            let timestamps: Vec<i64> = receipt.timestamps.into_iter().map(|t| t as i64).collect();
            if timestamps.is_empty() {
                return;
            }
            {
                let inner = core.inner.lock().await;
                let _ = inner
                    .store
                    .update_delivery_status(&conv_id, &timestamps, status)
                    .await;
            }
            for ts in timestamps {
                let _ = core.event_tx.send(IncomingEvent::ReceiptUpdate {
                    update: DeliveryStatusUpdate {
                        conversation_id: conv_id.clone(),
                        sent_at_ms: ts,
                        delivery_status: status,
                    },
                });
            }
        }
        Some(Body::Text(text)) => {
            let body = text.body;
            let sent_at = if msg.timestamp_ms > 0 {
                Some(msg.timestamp_ms as i64)
            } else {
                None
            };

            // Message-request gate (docs/12 §1) — DMs only; group text arrives
            // as `GroupMessage`, not here.
            if decrypted.group_id.is_none() {
                let inner = core.inner.lock().await;
                let gate = inner.sender_gate(&decrypted.sender_did).await;
                if gate.is_blocked {
                    // Decrypted to advance the ratchet, now dropped: no event,
                    // no notification, no delivery receipt (docs/12 §2).
                    return;
                }
                // Recency bump (non-curating). Un-curated human → pending
                // request flag so Delete can dismiss without curating.
                let _ = inner
                    .store
                    .touch_contact(&decrypted.sender_did, false, Timestamp::now())
                    .await;
                if !gate.passes() {
                    let _ = inner
                        .store
                        .set_pending_request(&decrypted.sender_did, true)
                        .await;
                }
            } else {
                let inner = core.inner.lock().await;
                let _ = inner
                    .store
                    .touch_contact(&decrypted.sender_did, false, Timestamp::now())
                    .await;
            }

            // Auto-send delivery receipt to the sender — DM only. Group
            // delivery receipts would fan out per-recipient and aren't part of
            // the group read-tracking model yet.
            if let (Some(ts), None) = (sent_at, decrypted.group_id.as_deref()) {
                let ws = core.ws.lock().expect("ws mutex poisoned").clone();
                let mut inner = core.inner.lock().await;
                let profile_key = inner.own_profile_key().await;
                let delivery = ContentMessage {
                    body: Some(Body::Receipt(ReceiptMessage {
                        r#type: receipt_message::Type::Delivery as i32,
                        timestamps: vec![ts as u64],
                    })),
                    timestamp_ms: 0,
                    profile_key,
                };
                let _ = inner
                    .send_dm(ws.as_ref(), &decrypted.sender_did, &delivery.encode_to_vec(), None)
                    .await;
            }

            let out = DecryptedMessage {
                plaintext: body.into_bytes(),
                sent_at_ms: sent_at,
                ..decrypted
            };
            let _ = core.event_tx.send(IncomingEvent::Message { msg: out });
        }
        Some(Body::GroupContext(ctx)) => {
            // Persist the group master key locally so `fetch_group_state`
            // works. Surface a typed `GroupInvite` event for the UI to
            // refresh its conversation list — do NOT surface a `Message`
            // event (the envelope plaintext isn't user-facing text; iOS
            // would render it as "(binary)"). Mirrors the SKDM handler
            // below: cryptographic plumbing, not content.
            let ws = core.ws.lock().expect("ws mutex poisoned").clone();
            let mut inner = core.inner.lock().await;
            let result = groups::store_inbound_group_context(
                &inner.store,
                &ctx.group_master_key,
                &ctx.hosting_server_url,
            )
            .await;
            match result {
                Ok(group_id) => {
                    // Auto-accept the invite. We don't expose an explicit
                    // "accept" affordance — receiving the master key is
                    // already an out-of-band trust signal from the inviter
                    // (matches Signal's group UX). `complete_join_group`
                    // fetches state, submits the accept action, seeds our
                    // own Sender Key, and DMs the SKDM to every existing
                    // member — without it, our first `send_group_message`
                    // fails with "missing sender key state for distribution ID".
                    if let Err(e) = inner
                        .complete_join_group(ws.as_ref(), &ctx.hosting_server_url, &group_id)
                        .await
                    {
                        tracing::warn!(
                            "[groups] auto-accept of invite for {group_id} failed: {e}"
                        );
                    }
                    drop(inner);
                    let _ = core.event_tx.send(IncomingEvent::GroupInvite {
                        group_id,
                        hosting_server_url: ctx.hosting_server_url.clone(),
                        inviter_did: decrypted.sender_did.clone(),
                    });
                }
                Err(e) => {
                    drop(inner);
                    tracing::warn!("[groups] failed to store inbound GroupContext: {e}");
                }
            }
        }
        Some(Body::SenderKeyDistribution(skdm_msg)) => {
            // Install the sender's group key locally so future
            // `GroupMessage`s from them decrypt.
            let mut inner = core.inner.lock().await;
            if let Err(e) = groups::process_inbound_skdm(
                &mut inner.store,
                &decrypted.sender_did,
                decrypted.sender_device_id,
                &skdm_msg.skdm,
            )
            .await
            {
                tracing::warn!("[groups] failed to process inbound SKDM: {e}");
            }
            // No app-level event — SKDM is plumbing, not content.
        }
        Some(Body::GroupMessage(gm)) => {
            // Decrypt under the sender's cached Sender Key; surface as a
            // normal Message event with plaintext substituted.
            let mut inner = core.inner.lock().await;
            match groups::decrypt_group_content(
                &mut inner.store,
                &decrypted.sender_did,
                decrypted.sender_device_id,
                &gm.ciphertext,
            )
            .await
            {
                Ok(plaintext) => {
                    // Group co-member traffic is a non-curating contact touch.
                    let _ = inner
                        .store
                        .touch_contact(&decrypted.sender_did, false, Timestamp::now())
                        .await;
                    let out = DecryptedMessage {
                        plaintext,
                        group_id: Some(groups::b64(&gm.group_id)),
                        ..decrypted
                    };
                    let _ = core.event_tx.send(IncomingEvent::Message { msg: out });
                }
                Err(e) => {
                    tracing::warn!("[groups] failed to decrypt GroupMessage: {e}");
                }
            }
        }
        Some(Body::TimerChange(timer)) => {
            // Silent control message — update local expiry setting, no chat event.
            let expiry = if timer.expiry_secs > 0 {
                Some(timer.expiry_secs)
            } else {
                None
            };
            let inner = core.inner.lock().await;
            let _ = inner
                .store
                .save_conversation_expiry(&decrypted.sender_did, expiry)
                .await;
        }
        Some(Body::Edit(edit)) => {
            let inner = core.inner.lock().await;
            if let Some(ev) = inner
                .apply_inbound_edit(&conv_id, &decrypted.sender_did, edit, msg.timestamp_ms)
                .await
            {
                drop(inner);
                let _ = core.event_tx.send(ev);
            }
        }
        Some(Body::Delete(del)) => {
            let inner = core.inner.lock().await;
            if let Some(ev) = inner
                .apply_inbound_delete(&conv_id, &decrypted.sender_did, del, msg.timestamp_ms)
                .await
            {
                drop(inner);
                let _ = core.event_tx.send(ev);
            }
        }
        Some(Body::Reaction(reaction)) => {
            let inner = core.inner.lock().await;
            if let Some(ev) = inner
                .apply_inbound_reaction(&conv_id, &decrypted.sender_did, reaction, msg.timestamp_ms)
                .await
            {
                drop(inner);
                let _ = core.event_tx.send(ev);
            }
        }
        None => {
            // ContentMessage with no body — emit as raw bytes (backward compat).
            let _ = core.event_tx.send(IncomingEvent::Message { msg: decrypted });
        }
    }
}

#[cfg(test)]
mod tests {
    use prost::Message as _;

    use crate::proto::{content_message::Body, ContentMessage, TimerChangeMessage};

    #[test]
    fn sender_gate_passes_for_curated_or_bot_only() {
        use crate::messaging::SenderGate;
        let g = |is_curated, is_blocked, is_bot| SenderGate { is_curated, is_blocked, is_bot };
        // Curated human delivers normally.
        assert!(g(true, false, false).passes());
        // Homeserver-known bot skips the gate even when un-curated (docs/12).
        assert!(g(false, false, true).passes());
        // Un-curated human is a request, not a pass.
        assert!(!g(false, false, false).passes());
        // Blocked is handled by the caller (dropped); `passes` only decides
        // request-vs-normal, so a curated+blocked row still "passes" the gate.
        assert!(g(true, true, false).passes());
    }

    #[test]
    fn timer_change_proto_round_trip() {
        let msg = ContentMessage {
            body: Some(Body::TimerChange(TimerChangeMessage { expiry_secs: 3600 })),
            timestamp_ms: 0,
            profile_key: vec![],
        };
        let encoded = msg.encode_to_vec();
        let decoded = ContentMessage::decode(encoded.as_slice()).unwrap();
        match decoded.body {
            Some(Body::TimerChange(t)) => assert_eq!(t.expiry_secs, 3600),
            other => panic!("unexpected body: {other:?}"),
        }
    }

    #[test]
    fn timer_change_zero_encodes_correctly() {
        // expiry_secs = 0 means "disable"; it must survive the proto round-trip
        // so the receiver can distinguish "set to 0" from "field absent".
        let msg = ContentMessage {
            body: Some(Body::TimerChange(TimerChangeMessage { expiry_secs: 0 })),
            timestamp_ms: 0,
            profile_key: vec![],
        };
        let decoded = ContentMessage::decode(msg.encode_to_vec().as_slice()).unwrap();
        match decoded.body {
            Some(Body::TimerChange(t)) => assert_eq!(t.expiry_secs, 0),
            other => panic!("unexpected body: {other:?}"),
        }
    }
}
