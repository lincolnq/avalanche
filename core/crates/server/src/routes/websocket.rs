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

use std::collections::HashMap;
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
        ws_frame::Body, DeliverRequest, Keepalive, PrekeyLowNotification, SendResponse, WsFrame,
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

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, device_pk)))
}

/// Outstanding server-initiated `DeliverRequest`s awaiting a `DeliverAck`.
/// Maps `frame.id` (chosen by the server when pushing) → queued message PK.
type PendingAcks = Arc<Mutex<HashMap<u64, i64>>>;

async fn handle_ws(socket: WebSocket, state: AppState, device_pk: i64) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WsPush>();

    state.ws_connections.write().await.insert(device_pk, tx);

    let pending: PendingAcks = Arc::new(Mutex::new(HashMap::new()));
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
                pending.lock().await.insert(frame_id, msg.id);

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
                        pending.lock().await.insert(frame_id, pd.message_id);
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
                    WsPush::PrekeyLow { one_time_remaining, kyber_remaining } => {
                        WsFrame {
                            id: 0,
                            body: Some(Body::PrekeyLow(PrekeyLowNotification {
                                one_time_remaining,
                                kyber_remaining,
                            })),
                        }
                    }
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
                        if let Some(reply) = handle_frame(&state, device_pk, &pending, frame).await {
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
}

/// Decode and dispatch a single client-originated frame. Returns the response
/// frame the server should send back, if any. Side effects (DB writes, ack
/// bookkeeping) happen inside.
async fn handle_frame(
    state: &AppState,
    device_pk: i64,
    pending: &PendingAcks,
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
            let msg_id = pending.lock().await.remove(&frame_id);
            if let Some(msg_id) = msg_id {
                if let Ok(mut conn) = state.db.acquire().await {
                    let _ = db::messages::acknowledge(&mut conn, device_pk, &[msg_id]).await;
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
        Body::SendResponse(_) | Body::DeliverRequest(_) | Body::PrekeyLow(_) => None,
    }
}

fn encode(frame: &WsFrame) -> Vec<u8> {
    let mut buf = Vec::with_capacity(frame.encoded_len());
    frame.encode(&mut buf).expect("encoding into Vec cannot fail");
    buf
}
