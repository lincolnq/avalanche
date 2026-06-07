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
    self, content_message::Body, receipt_message, ContentMessage, ReceiptMessage,
};
use crate::{
    AppCore, AppCoreInner, AppError, DecryptedMessage, DeliveryStatusUpdate, IncomingEvent,
};

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
            &mut self.store,
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

        let has_session = if force_refresh {
            false
        } else {
            use libsignal_protocol::SessionStore;
            let protocol_addr = libsignal_protocol::ProtocolAddress::new(
                recipient_sid.clone(),
                libsignal_protocol::DeviceId::try_from(recipient_device_id).unwrap(),
            );
            self.store
                .load_session(&protocol_addr)
                .await
                .map_err(|e| AppError::Crypto(crypto::CryptoError::Signal(e)))?
                .is_some()
        };

        if !has_session {
            let bundle = self
                .client
                .fetch_prekey_bundle(recipient_did, recipient_device_id as i32)
                .await?;

            let recipient_bundle = crypto::RecipientKeyBundle {
                identity_key: bundle.identity_key.clone(),
                registration_id: bundle.registration_id as u32,
                device_id: recipient_device_id,
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

            session::initiate_session(
                &mut self.store,
                &self.local_address,
                &recipient_addr,
                &recipient_bundle,
            )
            .await?;
        }

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
                            // Auto-send delivery receipt.
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
            let conv_id = {
                let inner = core.inner.lock().await;
                let conv_id = format!("dm-{}-{}", inner.did, decrypted.sender_did);
                let _ = inner
                    .store
                    .update_delivery_status(&conv_id, &timestamps, status)
                    .await;
                conv_id
            };
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

            // Touch the contact row on inbound traffic — non-curating, just
            // a recency bump so the People/Other autocomplete sorting works.
            {
                let inner = core.inner.lock().await;
                let _ = inner
                    .store
                    .touch_contact(&decrypted.sender_did, false, Timestamp::now())
                    .await;
            }

            // Auto-send delivery receipt to the sender.
            if let Some(ts) = sent_at {
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
        None => {
            // ContentMessage with no body — emit as raw bytes (backward compat).
            let _ = core.event_tx.send(IncomingEvent::Message { msg: decrypted });
        }
    }
}
