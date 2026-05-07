//! WebSocket client for real-time message delivery.
//!
//! Connects to `GET /v1/ws?token=<session_token>` on the homeserver and
//! receives encrypted messages as they arrive. This replaces HTTP polling
//! for real-time use cases.

use base64::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite;

use crate::error::NetError;
use crate::types::InboundMessage;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A WebSocket connection to a homeserver for real-time message delivery.
pub struct WsConnection {
    ws: WsStream,
}

#[derive(Deserialize)]
struct RawWsMessage {
    r#type: String,
    id: Option<i64>,
    ciphertext: Option<String>,
    message_kind: Option<i16>,
    sender_did: Option<String>,
    sender_device_id: Option<i32>,
}

impl WsConnection {
    /// Connect to the homeserver's WebSocket endpoint.
    ///
    /// The server will immediately drain any queued messages for this device.
    pub async fn connect(server_url: &str, token: &str) -> Result<Self, NetError> {
        let ws_url = server_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1);
        let url = format!("{}/v1/ws?token={}", ws_url, token);

        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| NetError::WebSocket(e.to_string()))?;

        Ok(Self { ws })
    }

    /// Wait for the next message from the server.
    ///
    /// Returns `Ok(None)` when the connection is closed.
    pub async fn next_message(&mut self) -> Result<Option<InboundMessage>, NetError> {
        loop {
            match self.ws.next().await {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    let raw: RawWsMessage = serde_json::from_str(text.as_str())
                        .map_err(|e| NetError::WebSocket(format!("invalid JSON: {e}")))?;

                    if raw.r#type == "message" {
                        let ciphertext = raw
                            .ciphertext
                            .as_deref()
                            .map(|s| BASE64_STANDARD.decode(s))
                            .transpose()
                            .map_err(|e| NetError::Base64(e.to_string()))?
                            .unwrap_or_default();

                        return Ok(Some(InboundMessage {
                            id: raw.id.unwrap_or(0),
                            ciphertext,
                            message_kind: raw.message_kind.unwrap_or(0),
                            enqueued_at: String::new(),
                            sender_did: raw.sender_did,
                            sender_device_id: raw.sender_device_id,
                        }));
                    }
                    // Skip non-message types (ping responses, etc.)
                }
                Some(Ok(tungstenite::Message::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => continue, // ping/pong/binary frames
                Some(Err(e)) => return Err(NetError::WebSocket(e.to_string())),
            }
        }
    }

    /// Acknowledge delivered messages so the server can delete them.
    pub async fn ack(&mut self, message_ids: &[i64]) -> Result<(), NetError> {
        let msg = serde_json::json!({
            "type": "ack",
            "message_ids": message_ids,
        });
        self.ws
            .send(tungstenite::Message::Text(msg.to_string().into()))
            .await
            .map_err(|e| NetError::WebSocket(e.to_string()))
    }
}
