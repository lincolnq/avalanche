//! WebSocket endpoint for real-time messaging.
//!
//! `GET /v1/ws?token=<session_token>` upgrades to a binary WebSocket carrying
//! `actnet.ws.WsFrame` protobuf frames (defined in `proto/ws.proto`). Either
//! side may originate requests; responses echo the originator's `frame.id`.
//!
//! Frames the server handles:
//!
//! - **`SendRequest`** (client → server): enqueue one or more outbound
//!   messages. Server replies with a `SendResponse` containing the assigned
//!   message IDs (or an error string + HTTP-like status on failure).
//! - **`DeliverAck`** (client → server): the client received the
//!   `DeliverRequest` whose `frame.id` matches; the server now deletes the
//!   queued row.
//! - **`Keepalive`** (either direction): echo back with the same `frame.id`.
//!
//! Frames the server originates:
//!
//! - **`DeliverRequest`** (server → client): an incoming message for this
//!   device. The server allocates a fresh `frame.id`, records the mapping
//!   to the queued message PK, sends the frame, and waits for a
//!   `DeliverAck` with that id before deleting from the queue.
//! - **`PrekeyLowNotification`** (server → client): prekey pool is below
//!   threshold; client should refill. Fire-and-forget; no response.
//!
//! # Security notes
//!
//! - Authentication is via `?token=` query parameter, validated before the
//!   upgrade completes. An invalid token gets HTTP 401, not a WebSocket
//!   connection. The token resolves to a `device_pk` once; every frame on
//!   the connection inherits that identity. There is no per-frame auth.
//! - The connection map (`ws_connections`) lives in-process. On restart all
//!   connections drop; clients reconnect and the on-connect drain replays
//!   any unacked queued messages.
//! - HTTP `POST /v1/messages` remains live as a fallback for clients that
//!   can't keep a WS open.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
    routing::get,
    Router,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

use crate::{
    db,
    error::ServerError,
    proto::{
        ws_frame::Body, AccountJoinedEvent, DeliverRequest, GroupDeliverRequest, Keepalive,
        PrekeyLowNotification, SendResponse, StorageChangedNotification, WsFrame,
    },
    routes::messages::{send_messages, SendInput},
    state::{AppState, WsPush},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/ws", get(ws_upgrade))
}

#[derive(Deserialize)]
struct WsQuery {
    token: String,
}

async fn ws_upgrade(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ServerError> {
    let mut conn = state.db.acquire().await?;
    let device_pk = db::sessions::validate(&mut conn, &query.token)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    // Resolve the device's account once at upgrade time and check whether it
    // holds `subscribe.account_joined` (the pinned adminbot Project's bots get
    // it via the superuser short-circuit). If so, this session receives
    // `AccountJoinedEvent` pushes for as long as it's connected.
    let account_id: Option<i64> = sqlx::query_scalar(
        "SELECT a.id FROM devices d JOIN accounts a ON d.account_id = a.id WHERE d.id = $1",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await
    .map_err(ServerError::Db)?;
    let subscribes_account_joined = match account_id {
        Some(aid) => {
            db::capabilities::account_has_capability(
                &mut conn,
                aid,
                db::capabilities::SUBSCRIBE_ACCOUNT_JOINED,
            )
            .await?
        }
        None => false,
    };

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(socket, state, device_pk, subscribes_account_joined)
    }))
}

/// Outstanding server-initiated pushes awaiting an ack. Tracks both
/// 1:1 (`DeliverRequest`/`DeliverAck`) and group
/// (`GroupDeliverRequest`/`GroupDeliverAck`) deliveries so the ack
/// handler knows which queue row to free.
enum PendingAck {
    Dm { message_id: i64 },
    Group { message_id: i64, pseudonym: Vec<u8> },
}
type PendingAcks = Arc<Mutex<HashMap<u64, PendingAck>>>;

async fn handle_ws(
    socket: WebSocket,
    state: AppState,
    device_pk: i64,
    subscribes_account_joined: bool,
) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WsPush>();

    state.ws_connections.write().await.insert(device_pk, tx.clone());
    if subscribes_account_joined {
        state
            .account_joined_subscribers
            .write()
            .await
            .insert(device_pk, tx.clone());
        tracing::info!(device_pk, "ws: account_joined subscriber connected");
    }

    let pending: PendingAcks = Arc::new(Mutex::new(HashMap::new()));
    // Pseudonyms this socket has subscribed to. Owned here so we can drop
    // them from `state.group_subscriptions` on disconnect even if the
    // client never sends an unsubscribe frame.
    let mut subscribed_pseudonyms: HashSet<Vec<u8>> = HashSet::new();
    // Server-allocated frame ids for its own outbound requests. Distinct id
    // space from client-originated ids; collisions across directions are
    // not possible because each side only ever interprets responses keyed
    // by ids it itself chose.
    let mut next_server_id: u64 = 1;

    // Drain any queued messages on connect.
    if let Ok(mut conn) = state.db.acquire().await {
        if let Ok(queued) = db::messages::fetch_for_device(&mut conn, device_pk).await {
            for msg in queued {
                let frame_id = next_server_id;
                next_server_id += 1;
                pending
                    .lock()
                    .await
                    .insert(frame_id, PendingAck::Dm { message_id: msg.id });

                let frame = WsFrame {
                    id: frame_id,
                    body: Some(Body::DeliverRequest(DeliverRequest {
                        message_id: msg.id,
                        ciphertext: msg.ciphertext,
                        message_kind: msg.message_kind as i32,
                        sender_did: msg.sender_did,
                        sender_device_id: msg.sender_device_id,
                        enqueued_at: Some(msg.enqueued_at.to_string()),
                    })),
                };
                if sink.send(Message::Binary(encode(&frame).into())).await.is_err() {
                    state.ws_connections.write().await.remove(&device_pk);
                    return;
                }
            }
        }
    }

    loop {
        tokio::select! {
            // Server-side push (DeliverRequest, PrekeyLow) coming in from
            // messages.rs or the prekey vacuum task.
            Some(push) = rx.recv() => {
                let frame = match push {
                    WsPush::Delivery(pd) => {
                        let frame_id = next_server_id;
                        next_server_id += 1;
                        pending
                            .lock()
                            .await
                            .insert(frame_id, PendingAck::Dm { message_id: pd.message_id });
                        WsFrame {
                            id: frame_id,
                            body: Some(Body::DeliverRequest(DeliverRequest {
                                message_id: pd.message_id,
                                ciphertext: pd.ciphertext,
                                message_kind: pd.message_kind as i32,
                                sender_did: pd.sender_did,
                                sender_device_id: pd.sender_device_id,
                                enqueued_at: pd.enqueued_at,
                            })),
                        }
                    }
                    WsPush::GroupDelivery(pgd) => {
                        let frame_id = next_server_id;
                        next_server_id += 1;
                        pending.lock().await.insert(
                            frame_id,
                            PendingAck::Group {
                                message_id: pgd.message_id,
                                pseudonym: pgd.recipient_group_pseudonym.clone(),
                            },
                        );
                        WsFrame {
                            id: frame_id,
                            body: Some(Body::GroupDeliverRequest(GroupDeliverRequest {
                                message_id: pgd.message_id,
                                group_id: pgd.group_id,
                                ciphertext: pgd.ciphertext,
                                recipient_group_pseudonym: pgd.recipient_group_pseudonym,
                                enqueued_at: pgd.enqueued_at,
                            })),
                        }
                    }
                    WsPush::PrekeyLow { one_time_remaining, kyber_remaining } => {
                        WsFrame {
                            id: 0,
                            body: Some(Body::PrekeyLow(PrekeyLowNotification {
                                one_time_remaining,
                                kyber_remaining,
                            })),
                        }
                    }
                    WsPush::AccountJoined { did, joined_at_ms, invite_token } => {
                        tracing::info!(
                            device_pk,
                            new_did = %did,
                            "ws: forwarding AccountJoined to subscriber"
                        );
                        WsFrame {
                            id: 0,
                            body: Some(Body::AccountJoined(AccountJoinedEvent {
                                did,
                                joined_at_ms,
                                invite_token,
                            })),
                        }
                    }
                    WsPush::StorageChanged { high_seq } => WsFrame {
                        id: 0,
                        body: Some(Body::StorageChanged(StorageChangedNotification {
                            high_seq,
                        })),
                    },
                };
                if sink.send(Message::Binary(encode(&frame).into())).await.is_err() {
                    break;
                }
            }

            // Inbound frames from the client.
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        let Ok(frame) = WsFrame::decode(bytes.as_ref()) else {
                            tracing::debug!(device_pk, "ws: failed to decode frame; ignoring");
                            continue;
                        };
                        if let Some(reply) = handle_frame(
                            &state,
                            device_pk,
                            &pending,
                            &tx,
                            &mut subscribed_pseudonyms,
                            frame,
                        )
                        .await
                        {
                            if sink.send(Message::Binary(encode(&reply).into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {} // text/ping/pong: ignore
                    Some(Err(e)) => {
                        tracing::debug!(device_pk, "ws: stream error: {e}");
                        break;
                    }
                }
            }
        }
    }

    state.ws_connections.write().await.remove(&device_pk);
    if subscribes_account_joined {
        let mut subs = state.account_joined_subscribers.write().await;
        // Only remove if it's still our channel (a later reconnect for the
        // same device may have overwritten it).
        if let Some(existing) = subs.get(&device_pk) {
            if existing.same_channel(&tx) {
                subs.remove(&device_pk);
                tracing::info!(device_pk, "ws: account_joined subscriber disconnected");
            }
        }
    }
    if !subscribed_pseudonyms.is_empty() {
        let mut subs = state.group_subscriptions.write().await;
        for p in &subscribed_pseudonyms {
            // Only remove the entry if it's still ours (a later connect for
            // the same pseudonym may have overwritten it).
            if let Some(existing) = subs.get(p) {
                if existing.same_channel(&tx) {
                    subs.remove(p);
                }
            }
        }
    }
}

/// Decode and dispatch a single client-originated frame. Returns the response
/// frame the server should send back, if any. Side effects (DB writes, ack
/// bookkeeping) happen inside.
async fn handle_frame(
    state: &AppState,
    device_pk: i64,
    pending: &PendingAcks,
    tx: &mpsc::UnboundedSender<WsPush>,
    subscribed_pseudonyms: &mut HashSet<Vec<u8>>,
    frame: WsFrame,
) -> Option<WsFrame> {
    let frame_id = frame.id;
    match frame.body? {
        Body::SendRequest(req) => {
            let inputs: Vec<SendInput> = req
                .messages
                .into_iter()
                .map(|m| SendInput {
                    recipient_did: m.recipient_did,
                    recipient_device_id: m.recipient_device_id,
                    destination_registration_id: m.destination_registration_id,
                    ciphertext: m.ciphertext,
                    message_kind: m.message_kind as i16,
                    expiry_secs: m.expiry_secs,
                })
                .collect();
            let resp = match send_messages(state, device_pk, &inputs).await {
                Ok(ids) => SendResponse {
                    message_ids: ids,
                    error: None,
                    status: 200,
                },
                Err(e) => {
                    let (status, msg) = match &e {
                        ServerError::RateLimited => (429, "rate limited".to_string()),
                        ServerError::NotFound => (404, "recipient device not found".to_string()),
                        ServerError::BadRequest(m) => (400, m.clone()),
                        ServerError::Unauthorized => (401, "unauthorized".to_string()),
                        _ => (500, "internal error".to_string()),
                    };
                    SendResponse {
                        message_ids: vec![],
                        error: Some(msg),
                        status,
                    }
                }
            };
            Some(WsFrame {
                id: frame_id,
                body: Some(Body::SendResponse(resp)),
            })
        }
        Body::DeliverAck(_) => {
            let entry = pending.lock().await.remove(&frame_id);
            if let Some(PendingAck::Dm { message_id }) = entry {
                if let Ok(mut conn) = state.db.acquire().await {
                    let _ = db::messages::acknowledge(&mut conn, device_pk, &[message_id]).await;
                }
            }
            None
        }
        Body::GroupDeliverAck(_) => {
            let entry = pending.lock().await.remove(&frame_id);
            if let Some(PendingAck::Group { message_id, pseudonym }) = entry {
                if let Ok(mut conn) = state.db.acquire().await {
                    let _ = db::group_messages::acknowledge(
                        &mut conn,
                        &pseudonym,
                        &[message_id],
                    )
                    .await;
                }
            }
            None
        }
        Body::SubscribeGroupPseudonyms(req) => {
            // Replace this socket's prior subscriptions. The request carries
            // the *full* desired set; anything we previously had that isn't
            // in the new list is dropped from both local tracking and the
            // server-wide subscription map.
            let new_set: HashSet<Vec<u8>> = req.pseudonyms.into_iter().collect();
            {
                let mut subs = state.group_subscriptions.write().await;
                // Drop any pseudonyms we previously held that aren't in the
                // new set (and that still point at this socket).
                for old in subscribed_pseudonyms.iter() {
                    if new_set.contains(old) {
                        continue;
                    }
                    if let Some(existing) = subs.get(old) {
                        if existing.same_channel(tx) {
                            subs.remove(old);
                        }
                    }
                }
                // Install the new set, overwriting any other connection
                // that previously held the same pseudonym (last writer
                // wins — same policy as `ws_connections`).
                for p in &new_set {
                    subs.insert(p.clone(), tx.clone());
                }
            }
            // Drain pending group messages for newly-subscribed pseudonyms
            // (anything in `new_set` that wasn't in `subscribed_pseudonyms`).
            let newly_added: Vec<Vec<u8>> = new_set
                .iter()
                .filter(|p| !subscribed_pseudonyms.contains(*p))
                .cloned()
                .collect();
            *subscribed_pseudonyms = new_set;
            if !newly_added.is_empty() {
                if let Ok(mut conn) = state.db.acquire().await {
                    for p in &newly_added {
                        if let Ok(queued) =
                            db::group_messages::fetch_for_pseudonym(&mut conn, p).await
                        {
                            for msg in queued {
                                let _ = tx.send(WsPush::GroupDelivery(
                                    crate::state::PendingGroupDelivery {
                                        message_id: msg.id,
                                        group_id: msg.group_id,
                                        ciphertext: msg.ciphertext,
                                        recipient_group_pseudonym: p.clone(),
                                        enqueued_at: Some(msg.enqueued_at.to_string()),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            None
        }
        Body::Keepalive(_) => Some(WsFrame {
            id: frame_id,
            body: Some(Body::Keepalive(Keepalive {})),
        }),
        // Variants the server should not receive from the client. Silently
        // ignore; never reflect server-only frames back.
        Body::SendResponse(_)
        | Body::DeliverRequest(_)
        | Body::GroupDeliverRequest(_)
        | Body::PrekeyLow(_)
        | Body::AccountJoined(_)
        | Body::StorageChanged(_) => None,
    }
}

fn encode(frame: &WsFrame) -> Vec<u8> {
    let mut buf = Vec::with_capacity(frame.encoded_len());
    frame.encode(&mut buf).expect("encoding into Vec cannot fail");
    buf
}
