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
/// Max time a single connect attempt (lazy auth + WS handshake) may take before
/// we treat it as failed and fall through to backoff. Without this, a stale
/// network path after the app resumes from suspension can leave the attempt
/// awaiting forever, pinning `Connecting` ("Reconnecting…") until a full
/// restart. (The reqwest client carries its own connect/request timeouts too;
/// this bounds the whole attempt including the WS handshake.)
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

pub(crate) async fn reconnect_loop(weak: std::sync::Weak<AppCore>) {
    let mut backoff_sec: u64 = 1;
    loop {
        let Some(core) = weak.upgrade() else {
            return;
        };

        core.publish_state(ConnectionState::Connecting);

        match tokio::time::timeout(CONNECT_TIMEOUT, try_connect_ws(&core)).await {
            Ok(Ok(ws)) => {
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
            Ok(Err(e)) => {
                tracing::warn!("[ws] connect failed: {e}");
            }
            Err(_elapsed) => {
                tracing::warn!("[ws] connect timed out after {CONNECT_TIMEOUT:?}");
            }
        }

        // Jittered backoff (0.75x – 1.25x) so reinstalls / mass-restarts don't
        // hammer the server in sync.
        let jitter = rand::random::<f64>() * 0.5 + 0.75;
        let sleep_ms = ((backoff_sec as f64) * 1000.0 * jitter) as u64;
        let next_attempt_at_ms = Timestamp::now().as_millis() + sleep_ms as i64;
        core.publish_state(ConnectionState::Reconnecting { next_attempt_at_ms });

        // Clone the wake signal before releasing the strong ref so AppCore can
        // drop while we sleep.
        let reconnect_notify = core.reconnect_notify.clone();
        drop(core);
        // Wait out the backoff, but wake early on an opportunistic-reconnect
        // signal (app returned to foreground, etc.) and retry immediately with a
        // reset backoff.
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)) => {
                backoff_sec = (backoff_sec * 2).min(30);
            }
            _ = reconnect_notify.notified() => {
                backoff_sec = 1;
            }
        }
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
    let store = { core.inner.lock().await.store.clone() };
    subscribe_all_group_pseudonyms(&store, Some(ws)).await
}

/// Subscribe the live WS to the *full* set of group push pseudonyms we hold
/// locally. The server treats `SubscribeGroupPseudonyms` as a REPLACE of the
/// socket's entire subscription set (`routes/websocket.rs`) — it installs the
/// frame's list and drops anything absent. So every code path that registers a
/// new group pseudonym (join / accept / create / reconcile) must re-send the
/// *complete* set through here: sending only the newly-joined pseudonym would
/// silently unsubscribe every other group, so their fan-outs stop arriving
/// until the next full reconnect. A no-op if `ws` is `None` (the connect-time
/// call in `reconnect_loop` is the backstop) or we hold no pseudonyms.
pub(crate) async fn subscribe_all_group_pseudonyms(
    store: &store::DeviceStore,
    ws: Option<&net::ws::WsConnection>,
) -> Result<(), AppError> {
    let Some(ws) = ws else {
        return Ok(());
    };
    let pseudonyms: Vec<Vec<u8>> = store
        .list_groups()
        .await?
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
    // Clone the `Client` out from under the inner lock, then release the lock
    // before any network I/O. This is load-bearing for cold-start latency: the
    // lazy challenge/response in `ensure_authenticated` below can block for up
    // to the connect timeout when the homeserver is unreachable, and holding
    // `inner` across it would serialize every local store read (loading the
    // conversation list, display names) behind an offline account — leaving the
    // chats list spinning on launch. The clone is cheap (reqwest is Arc-backed;
    // the token cache is shared via `Arc<Mutex>`), so the token obtained here is
    // still visible to the original client and to concurrent `send_*` calls.
    let client = {
        let inner = core.inner.lock().await;
        inner.client.clone()
    };

    let url = client.server_url().to_string();
    client.ensure_authenticated().await?;
    let token = client
        .token()
        .ok_or_else(|| AppError::Protocol("no session token after auth".into()))?;

    let ws = net::ws::WsConnection::connect(
        &url,
        &token,
        core.app_active.clone(),
        core.reconnect_notify.clone(),
    )
    .await?;
    Ok(ws)
}

/// Pull events off an open WebSocket until it errors or closes. Fans in
/// `DeliverRequest`s (1:1 messages, decrypted + emitted via
/// `process_decrypted`) and `AccountJoinedEvent`s (admin push surfaced on
/// the separate admin queue as `AdminEvent::AccountJoined`).
async fn run_receive_loop(core: &AppCore, ws: &net::ws::WsConnection) {
    loop {
        tokio::select! {
            // A group was joined/left/reconciled on this live connection. Re-send
            // the full pseudonym set so the new group starts receiving fan-outs
            // immediately, without waiting for the next reconnect. The connect-time
            // subscribe only runs once, over the groups that existed at connect;
            // this arm keeps the subscription in sync as the group set changes.
            _ = core.groups_changed.notified() => {
                let store = { core.inner.lock().await.store.clone() };
                if let Err(e) = subscribe_all_group_pseudonyms(&store, Some(ws)).await {
                    tracing::warn!("[ws] re-subscribe on group change failed: {e}");
                }
            }
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
