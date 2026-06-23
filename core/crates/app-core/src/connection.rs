//! Background reconnect / receive loop for the homeserver WebSocket.
//!
//! Owned by `AppCore::reconnect_task` (spawned in `start_reconnect_task`).
//! Holds a `Weak<AppCore>` so the task self-exits when the last strong
//! reference is dropped.

use types::Timestamp;

use crate::messaging::process_decrypted;
use crate::{AdminEvent, AppCore, AppError, ConnectionState, IncomingEvent};

/// Connect-receive-backoff loop. Runs as a background tokio task owned by
/// `AppCore::reconnect_task`. Holds a `Weak<AppCore>` so dropping the last
/// strong reference lets the task exit on its next iteration.
pub(crate) async fn reconnect_loop(weak: std::sync::Weak<AppCore>) {
    let mut backoff_sec: u64 = 1;
    loop {
        let Some(core) = weak.upgrade() else {
            return;
        };

        core.publish_state(ConnectionState::Connecting);

        match try_connect_ws(&core).await {
            Ok(ws) => {
                core.publish_state(ConnectionState::Connected);
                *core.ws.lock().expect("ws mutex poisoned") = Some(ws.clone());
                let connected_at = std::time::Instant::now();
                // Subscribe to all known group push pseudonyms so the
                // server pushes group fan-outs live (no group/v1/fetch
                // poll required). Without this, group sends sit in the
                // server queue and recipients only see them on an
                // explicit `fetch_group_messages` call.
                if let Err(e) = subscribe_group_pseudonyms(&core, &ws).await {
                    tracing::warn!("[ws] group-pseudonym subscribe failed: {e}");
                }
                // Proactively top up one-time prekey pools if the server says
                // we're low — covers draining while offline or a missed
                // `PrekeyLow` push. (The push handles draining while connected.)
                crate::prekeys::replenish_if_low(&core).await;
                run_receive_loop(&core, &ws).await;
                *core.ws.lock().expect("ws mutex poisoned") = None;
                // Only reset backoff if the connection was actually usable.
                // A handshake that succeeds but disconnects within a second
                // counts as a failure for backoff purposes — otherwise we
                // bounce 1s,2s,4s,1s,2s,4s indefinitely against a flapping
                // server.
                if connected_at.elapsed() >= std::time::Duration::from_secs(5) {
                    backoff_sec = 1;
                } else {
                    tracing::debug!(
                        "[ws] connection dropped after {:?}, not resetting backoff",
                        connected_at.elapsed()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("[ws] connect failed: {e}");
            }
        }

        // Jittered backoff (0.75x – 1.25x) so reinstalls / mass-restarts don't
        // hammer the server in sync.
        let jitter = rand::random::<f64>() * 0.5 + 0.75;
        let sleep_ms = ((backoff_sec as f64) * 1000.0 * jitter) as u64;
        let next_attempt_at_ms = Timestamp::now().as_millis() + sleep_ms as i64;
        core.publish_state(ConnectionState::Reconnecting { next_attempt_at_ms });

        // Release the strong ref before sleeping so AppCore can drop.
        drop(core);
        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        backoff_sec = (backoff_sec * 2).min(30);
    }
}

/// Send a `SubscribeGroupPseudonyms` frame listing every
/// `group_push_pseudonym` we've registered (one per group we belong to).
/// Each entry is the pseudonym bytes the server uses to look up where to
/// push group fan-outs.
async fn subscribe_group_pseudonyms(
    core: &AppCore,
    ws: &net::ws::WsConnection,
) -> Result<(), AppError> {
    let groups = {
        let inner = core.inner.lock().await;
        inner.store.list_groups().await?
    };
    let pseudonyms: Vec<Vec<u8>> = groups
        .into_iter()
        .filter_map(|g| g.group_push_pseudonym)
        .collect();
    if pseudonyms.is_empty() {
        return Ok(());
    }
    ws.subscribe_group_pseudonyms(pseudonyms)
        .map_err(|e| AppError::Protocol(format!("subscribe group pseudonyms: {e}")))?;
    Ok(())
}

/// Open a fresh WebSocket connection. Triggers lazy challenge/response on
/// the underlying `net::Client` if no token is present.
async fn try_connect_ws(core: &AppCore) -> Result<net::ws::WsConnection, AppError> {
    // Hold the lock just long enough to clone what we need, then release
    // before doing network I/O. Avoids blocking parallel send_dm calls.
    let url = {
        let inner = core.inner.lock().await;
        inner.client.server_url().to_string()
    };

    // ensure_authenticated needs to call client.challenge/authenticate.
    // It manages its own auth lock internally; we just hold the inner lock
    // long enough to call into it. Inside ensure_authenticated, network
    // I/O happens without holding inner's lock (because the auth mutex is
    // separate — see net::Client).
    let inner = core.inner.lock().await;
    inner.client.ensure_authenticated().await?;
    let token = inner
        .client
        .token()
        .ok_or_else(|| AppError::Protocol("no session token after auth".into()))?;
    drop(inner);

    let ws = net::ws::WsConnection::connect(&url, &token).await?;
    Ok(ws)
}

/// Pull events off an open WebSocket until it errors or closes. Fans in
/// `DeliverRequest`s (1:1 messages, decrypted + emitted via
/// `process_decrypted`) and `AccountJoinedEvent`s (admin push surfaced on
/// the separate admin queue as `AdminEvent::AccountJoined`).
async fn run_receive_loop(core: &AppCore, ws: &net::ws::WsConnection) {
    loop {
        tokio::select! {
            delivery = ws.next_message() => {
                let delivery = match delivery {
                    Ok(Some(d)) => d,
                    Ok(None) => {
                        tracing::debug!("[ws] connection closed cleanly");
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("[ws] receive error: {e}");
                        return;
                    }
                };

                // Decrypt under the inner lock; release before any further work.
                let decrypted = {
                    let mut inner = core.inner.lock().await;
                    match inner.decrypt_inbound(&delivery.message).await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(
                                "[ws] failed to decrypt message {} from {:?}: {}, acking to skip",
                                delivery.message.id,
                                delivery.message.sender_did,
                                e
                            );
                            drop(inner);
                            let _ = ws.ack(delivery.ack_token).await;
                            continue;
                        }
                    }
                };

                // Ack on the wire so the server stops re-delivering it.
                let _ = ws.ack(delivery.ack_token).await;

                // Parse the content envelope and emit appropriate IncomingEvents.
                process_decrypted(core, decrypted).await;
            }
            joined = ws.next_account_joined() => {
                match joined {
                    Ok(Some(e)) => {
                        let _ = core.admin_event_tx.send(AdminEvent::AccountJoined {
                            did: e.did,
                            joined_at_ms: e.joined_at_ms,
                        });
                    }
                    Ok(None) => {
                        tracing::debug!("[ws] connection closed cleanly (account_joined)");
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("[ws] receive error (account_joined): {e}");
                        return;
                    }
                }
            }
            low = ws.next_prekey_low() => {
                match low {
                    // Server says a pool is low while we're connected — top up.
                    Ok(Some(_)) => crate::prekeys::replenish_if_low(core).await,
                    Ok(None) => {
                        tracing::debug!("[ws] connection closed cleanly (prekey_low)");
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("[ws] receive error (prekey_low): {e}");
                        return;
                    }
                }
            }
            changed = ws.next_storage_changed() => {
                match changed {
                    // Durable storage changed on another of our devices (docs/05
                    // §8). Delta-pull now instead of waiting for the safety-net
                    // poll. The pull is cursor-keyed and idempotent, so a
                    // redundant nudge is harmless; surface a refresh if it applied
                    // anything.
                    Ok(Some(_)) => {
                        match core.sync_storage_async().await {
                            Ok(()) => {
                                let _ = core.event_tx.send(IncomingEvent::StorageSynced);
                            }
                            Err(e) => tracing::warn!("[ws] storage_changed pull failed: {e}"),
                        }
                    }
                    Ok(None) => {
                        tracing::debug!("[ws] connection closed cleanly (storage_changed)");
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("[ws] receive error (storage_changed): {e}");
                        return;
                    }
                }
            }
            group_delivery = ws.next_group_message() => {
                let delivery = match group_delivery {
                    Ok(Some(d)) => d,
                    Ok(None) => {
                        tracing::debug!("[ws] connection closed cleanly (group)");
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("[ws] receive error (group): {e}");
                        return;
                    }
                };

                let decrypted = {
                    let mut inner = core.inner.lock().await;
                    match inner.process_inbound_group_delivery(&delivery).await {
                        Ok(d) => Some(d),
                        Err(e) => {
                            tracing::warn!(
                                "[ws] failed to process group delivery msg_id={}: {e}, acking to skip",
                                delivery.message_id
                            );
                            None
                        }
                    }
                };

                // Always ack so the server stops re-pushing — even on
                // decrypt failure, the message is unrecoverable for this
                // session.
                let _ = ws.group_ack(delivery.ack_token).await;

                if let Some(msg) = decrypted {
                    // Group content now rides a `ContentMessage` envelope just
                    // like DMs, so dispatch through the same decoder: text →
                    // Message, plus reactions/edits/deletes. Raw-text legacy
                    // group messages fall back to a plain Message inside
                    // `process_decrypted`.
                    process_decrypted(core, msg).await;
                }
            }
        }
    }
}
