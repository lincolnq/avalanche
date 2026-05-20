//! Encrypted message endpoints.
//!
//! - `POST /v1/messages` — send encrypted messages to recipient devices.
//!   If the recipient has a live WebSocket, the ciphertext is pushed
//!   immediately. Otherwise it is queued for later retrieval.
//! - `GET /v1/messages` — drain queued messages (used on reconnect).
//! - `DELETE /v1/messages` — acknowledge delivered messages so the server
//!   can delete them.
//!
//! # Security notes
//!
//! - **The server never sees plaintext.** The `ciphertext` field is an opaque
//!   blob encrypted client-side with the Double Ratchet. The server stores
//!   and forwards it without decryption.
//! - **Message send records the sender's account and device.** This is routing
//!   metadata the server necessarily knows. The sender columns are nullable
//!   to support future sealed-sender mode (Stage 4+) where the server cannot
//!   identify the sender.
//! - **Acknowledge is scoped.** `DELETE /v1/messages` only deletes messages
//!   belonging to the authenticated device, preventing a client from
//!   deleting another device's queue.
//! - **Message expiry is server-enforced.** Each message has an `expires_at`
//!   timestamp set at enqueue time. A background task garbage-collects
//!   expired messages. This is defense-in-depth alongside client-side expiry.

use axum::{
    extract::State,
    routing::{delete, get, post},
    Json, Router,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState, state::WsMessage};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/messages", post(send))
        .route("/v1/messages", get(fetch))
        .route("/v1/messages", delete(acknowledge))
}

// ── Send ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SendRequest {
    messages: Vec<OutboundMessage>,
}

#[derive(Deserialize)]
struct OutboundMessage {
    recipient_did: String,
    recipient_device_id: i32,
    ciphertext: String, // base64
    message_kind: i16,
}

#[derive(Serialize)]
struct SendResponse {
    sent: Vec<i64>,
}

async fn send(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    // Look up sender's account ID and device_id for metadata columns.
    let row = sqlx::query(
        "SELECT d.account_id, a.did AS sender_did, d.device_id AS sender_device_id
         FROM devices d JOIN accounts a ON a.id = d.account_id
         WHERE d.id = $1",
    )
    .bind(auth.device_pk)
    .fetch_one(&mut *conn)
    .await?;
    let sender_account_id: i64 = row.get("account_id");
    let sender_did: String = row.get("sender_did");
    let sender_device_id: i32 = row.get("sender_device_id");

    if !db::rate_limits::check_and_increment(
        &mut conn,
        sender_account_id,
        crate::middleware::rate_limit::ACTION_SEND_MESSAGE,
        crate::middleware::rate_limit::LIMIT_SEND_MESSAGE,
        crate::middleware::rate_limit::WINDOW_SEND_MESSAGE,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let mut sent = Vec::with_capacity(req.messages.len());
    let mut ws_pushes = Vec::new();

    for msg in &req.messages {
        let ciphertext = BASE64_STANDARD
            .decode(&msg.ciphertext)
            .map_err(|_| ServerError::BadRequest("invalid base64 ciphertext".into()))?;

        let device = db::devices::find_by_did(&mut conn, &msg.recipient_did, msg.recipient_device_id)
            .await?
            .ok_or(ServerError::NotFound)?;

        let msg_id = db::messages::enqueue(
            &mut conn,
            device.id,
            Some(sender_account_id),
            Some(auth.device_pk),
            &ciphertext,
            msg.message_kind,
            state.config.message_expiry_secs,
        )
        .await?;

        ws_pushes.push((device.id, msg_id, ciphertext, msg.message_kind));
        sent.push(msg_id);
    }

    // Push over WebSocket after all messages are persisted.
    // Collect offline device PKs for push relay wakeup.
    let mut offline_device_pks = Vec::new();

    for (device_id, msg_id, ciphertext, message_kind) in ws_pushes {
        let ws_conns = state.ws_connections.read().await;
        if let Some(tx) = ws_conns.get(&device_id) {
            let ws_msg = serde_json::json!({
                "type": "message",
                "id": msg_id,
                "ciphertext": BASE64_STANDARD.encode(&ciphertext),
                "message_kind": message_kind,
                "sender_did": sender_did,
                "sender_device_id": sender_device_id,
            });
            let _ = tx.send(WsMessage(ws_msg.to_string()));
        } else {
            offline_device_pks.push(device_id);
        }
    }

    // Send push wakeups for offline devices (best-effort, don't block response).
    if !offline_device_pks.is_empty() {
        if let Some(relay_url) = &state.config.relay_url {
            let relay_url = relay_url.clone();
            let db = state.db.clone();
            tokio::spawn(async move {
                send_push_wakeups(&relay_url, &db, &offline_device_pks).await;
            });
        }
    }

    Ok(Json(SendResponse { sent }))
}

// ── Fetch (reconnect drain) ──────────────────────────────────────────────────

#[derive(Serialize)]
struct FetchResponse {
    messages: Vec<MessageWire>,
}

#[derive(Serialize)]
struct MessageWire {
    id: i64,
    ciphertext: String,
    message_kind: i16,
    enqueued_at: String,
    sender_did: Option<String>,
    sender_device_id: Option<i32>,
}

async fn fetch(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<Json<FetchResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let queued = db::messages::fetch_for_device(&mut conn, auth.device_pk).await?;

    let messages = queued
        .into_iter()
        .map(|m| MessageWire {
            id: m.id,
            ciphertext: BASE64_STANDARD.encode(&m.ciphertext),
            message_kind: m.message_kind,
            enqueued_at: m.enqueued_at.to_string(),
            sender_did: m.sender_did,
            sender_device_id: m.sender_device_id,
        })
        .collect();

    Ok(Json(FetchResponse { messages }))
}

// ── Acknowledge ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AckRequest {
    message_ids: Vec<i64>,
}

async fn acknowledge(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<AckRequest>,
) -> Result<(), ServerError> {
    let mut conn = state.db.acquire().await?;
    db::messages::acknowledge(&mut conn, auth.device_pk, &req.message_ids).await?;
    Ok(())
}

// ── Push relay wakeup ───────────────────────────────────────────────────────

/// Look up push pseudonyms for offline devices and send wakeup pings to the
/// push relay. Best-effort: failures are logged but don't affect message delivery.
async fn send_push_wakeups(relay_url: &str, db: &sqlx::PgPool, device_pks: &[i64]) {
    let mut pseudonyms = Vec::new();

    for &device_pk in device_pks {
        let mut conn = match db.acquire().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("push: failed to acquire db connection: {}", e);
                return;
            }
        };
        match db::push::pseudonym_for_device(&mut conn, device_pk).await {
            Ok(Some(p)) => pseudonyms.push(p),
            Ok(None) => {
                tracing::debug!(device_pk, "push: no pseudonym registered, skipping");
            }
            Err(e) => {
                tracing::warn!(device_pk, "push: failed to look up pseudonym: {}", e);
            }
        }
    }

    if pseudonyms.is_empty() {
        return;
    }

    tracing::info!(count = pseudonyms.len(), "push: sending wakeup to relay");

    let body = serde_json::json!({ "pseudonyms": pseudonyms });
    match reqwest::Client::new()
        .post(format!("{}/v1/wakeup", relay_url))
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::debug!("push: relay wakeup succeeded");
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!("push: relay returned {}: {}", status, text);
        }
        Err(e) => {
            tracing::warn!("push: failed to reach relay: {}", e);
        }
    }
}
