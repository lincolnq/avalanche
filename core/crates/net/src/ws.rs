//! WebSocket client for real-time messaging.
//!
//! Connects to `GET /v1/ws?token=<session_token>` on the homeserver and
//! exchanges `actnet.ws.WsFrame` protobuf frames (binary). The connection
//! supports three flows:
//!
//! - **Incoming deliveries.** The server pushes `DeliverRequest` frames as
//!   messages arrive (and on connect, drains anything that queued while
//!   offline). [`WsConnection::next_message`] yields each one. The caller
//!   must call [`WsConnection::ack`] with the returned `ack_token` after
//!   processing, so the server can delete the row.
//! - **Outgoing sends.** [`WsConnection::send_messages`] dispatches a
//!   `SendRequest` and awaits the server's `SendResponse` (matched by
//!   correlation id), returning the assigned message IDs.
//! - **Keepalive.** The server's keepalive requests are echoed back
//!   automatically by the reader task.
//!
//! The connection is internally split into a reader task and a writer task,
//! sharing an outbound mpsc and a `frame.id → oneshot::Sender<SendResponse>`
//! correlation map. [`WsConnection`] is `Clone` (Arc-backed) so the same
//! connection can be used concurrently from a receive loop and a sender.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite;

use crate::error::NetError;
use crate::proto::{
    ws_frame::Body, DeliverAck, Keepalive, OutboundMessage, SendRequest, SendResponse, WsFrame,
};
use crate::types::InboundMessage;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A delivered message, paired with the ack token the caller must echo
/// back via [`WsConnection::ack`] for the server to delete the row.
pub struct InboundDelivery {
    pub message: InboundMessage,
    /// Opaque correlation token. Pass unchanged to [`WsConnection::ack`].
    pub ack_token: u64,
}

/// A handle to an open WebSocket connection. Clone freely — internal state
/// is Arc-shared and methods take `&self`.
#[derive(Clone)]
pub struct WsConnection {
    inner: Arc<Inner>,
}

struct Inner {
    /// Outbound frame queue, drained by the writer task.
    outbound: mpsc::UnboundedSender<Vec<u8>>,
    /// Incoming `DeliverRequest`s, drained by `next_message`.
    deliveries: Mutex<mpsc::UnboundedReceiver<InboundDelivery>>,
    /// Pending `SendRequest`s awaiting a response, keyed by frame.id.
    correlations: Mutex<HashMap<u64, oneshot::Sender<SendResponse>>>,
    /// Client-side correlation id counter. Starts at 1; 0 is reserved for
    /// fire-and-forget frames (matches server convention).
    next_id: AtomicU64,
}

impl WsConnection {
    /// Connect to the homeserver's WebSocket endpoint. Spawns background
    /// reader and writer tasks that live until the connection closes.
    pub async fn connect(server_url: &str, token: &str) -> Result<Self, NetError> {
        let ws_url = server_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1);
        let url = format!("{}/v1/ws?token={}", ws_url, token);

        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| NetError::WebSocket(e.to_string()))?;

        let (sink, stream) = ws.split();
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (delivery_tx, delivery_rx) = mpsc::unbounded_channel::<InboundDelivery>();

        let inner = Arc::new(Inner {
            outbound: outbound_tx.clone(),
            deliveries: Mutex::new(delivery_rx),
            correlations: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        spawn_writer(sink, outbound_rx);
        spawn_reader(stream, outbound_tx, delivery_tx, inner.clone());

        Ok(Self { inner })
    }

    /// Wait for the next incoming message. Returns `Ok(None)` when the
    /// connection is closed and no more frames will arrive.
    pub async fn next_message(&self) -> Result<Option<InboundDelivery>, NetError> {
        Ok(self.inner.deliveries.lock().await.recv().await)
    }

    /// Acknowledge a delivered message. The server uses the `ack_token`
    /// (the originating `DeliverRequest`'s frame.id) to look up which
    /// queued row to delete.
    pub async fn ack(&self, ack_token: u64) -> Result<(), NetError> {
        let frame = WsFrame {
            id: ack_token,
            body: Some(Body::DeliverAck(DeliverAck {})),
        };
        send_frame(&self.inner.outbound, &frame)
    }

    /// Send a batch of encrypted messages over the WebSocket and wait for
    /// the server's response. Returns the assigned message IDs in input
    /// order, or an error if the server reported a failure.
    pub async fn send_messages(
        &self,
        messages: Vec<OutboundMessage>,
    ) -> Result<Vec<i64>, NetError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.inner.correlations.lock().await.insert(id, tx);

        let frame = WsFrame {
            id,
            body: Some(Body::SendRequest(SendRequest { messages })),
        };
        if let Err(e) = send_frame(&self.inner.outbound, &frame) {
            self.inner.correlations.lock().await.remove(&id);
            return Err(e);
        }

        match rx.await {
            Ok(resp) => {
                if let Some(err) = resp.error {
                    Err(NetError::WebSocket(format!(
                        "server rejected send: {err} (status {})",
                        resp.status
                    )))
                } else {
                    Ok(resp.message_ids)
                }
            }
            // Sender dropped → reader task exited → connection closed.
            Err(_) => Err(NetError::WebSocket("connection closed".into())),
        }
    }
}

fn send_frame(
    outbound: &mpsc::UnboundedSender<Vec<u8>>,
    frame: &WsFrame,
) -> Result<(), NetError> {
    let mut buf = Vec::with_capacity(frame.encoded_len());
    frame
        .encode(&mut buf)
        .expect("encoding into Vec cannot fail");
    outbound
        .send(buf)
        .map_err(|_| NetError::WebSocket("connection closed".into()))
}

fn spawn_writer(
    mut sink: futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    mut outbound: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    tokio::spawn(async move {
        while let Some(bytes) = outbound.recv().await {
            if sink
                .send(tungstenite::Message::Binary(bytes.into()))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = sink.close().await;
    });
}

fn spawn_reader(
    mut stream: futures_util::stream::SplitStream<WsStream>,
    outbound: mpsc::UnboundedSender<Vec<u8>>,
    delivery_tx: mpsc::UnboundedSender<InboundDelivery>,
    state: Arc<Inner>,
) {
    tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let bytes = match frame {
                Ok(tungstenite::Message::Binary(b)) => b,
                Ok(tungstenite::Message::Close(_)) | Err(_) => break,
                // Ignore text frames (legacy) and ping/pong (handled by tungstenite).
                Ok(_) => continue,
            };
            let Ok(ws_frame) = WsFrame::decode(bytes.as_ref()) else {
                continue;
            };
            let id = ws_frame.id;
            let Some(body) = ws_frame.body else {
                continue;
            };
            match body {
                Body::DeliverRequest(d) => {
                    let _ = delivery_tx.send(InboundDelivery {
                        message: InboundMessage {
                            id: d.message_id,
                            ciphertext: d.ciphertext,
                            message_kind: d.message_kind as i16,
                            enqueued_at: d.enqueued_at.unwrap_or_default(),
                            sender_did: d.sender_did,
                            sender_device_id: d.sender_device_id,
                        },
                        ack_token: id,
                    });
                }
                Body::SendResponse(resp) => {
                    if let Some(tx) = state.correlations.lock().await.remove(&id) {
                        let _ = tx.send(resp);
                    }
                }
                Body::Keepalive(_) => {
                    let reply = WsFrame {
                        id,
                        body: Some(Body::Keepalive(Keepalive {})),
                    };
                    let _ = send_frame(&outbound, &reply);
                }
                // Server-side notifications without a response. Surface to
                // the delivery channel later if we add a typed event API;
                // ignored for now since no client consumer exists.
                Body::PrekeyLow(_) => {}
                // Variants the client should never receive from the server.
                Body::SendRequest(_) | Body::DeliverAck(_) => {}
            }
        }

        // Connection closed: drop the delivery sender so next_message returns
        // None, and fail any pending correlations.
        drop(delivery_tx);
        let mut map = state.correlations.lock().await;
        for (_, tx) in map.drain() {
            drop(tx);
        }
    });
}
