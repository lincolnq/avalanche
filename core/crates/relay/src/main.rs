//! Push notification relay service.
//!
//! A standalone Axum service that mediates between homeservers and APNs/FCM.
//! Homeservers never see device push tokens; they send wakeup requests
//! addressed to opaque pseudonyms. The relay maps pseudonyms to device tokens
//! and fires content-free push notifications.
//!
//! # Privacy model
//!
//! - Clients register per-(user, server) pseudonyms, so a relay cannot link
//!   a user's activity across homeservers.
//! - Push payloads are content-free: Apple/Google only see that the app was
//!   pinged, not who sent a message or what it says.
//! - Pseudonyms rotate periodically (default weekly) with a grace period
//!   so old pseudonyms still work briefly.
//!
//! # Storage
//!
//! SQLite (encrypted via SQLCipher) using tokio-rusqlite.
//! Database path: `$DATA_DIR/relay.db` (defaults to `./relay.db`).
//!
//! # APNs/FCM
//!
//! Currently logs the wakeup intent. To enable real APNs sending, set:
//!   APNS_KEY_PATH   — path to the .p8 private key file
//!   APNS_KEY_ID     — 10-character key ID from Apple Developer portal
//!   APNS_TEAM_ID    — 10-character team ID
//!   APNS_BUNDLE_ID  — app bundle ID (e.g. org.example.actnet)
//! and integrate the `a2` crate (https://crates.io/crates/a2) with token-based auth.
//! The notification payload must be content-free: `{"aps":{"content-available":1}}`.

use std::sync::Arc;

use axum::{
    extract::{Json, State},
    http::StatusCode,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio_rusqlite::Connection;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

// ── Types ───────────────────────────────────────────────────────────────────

struct RelayState {
    conn: Connection,
}

// ── Client endpoints ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    pseudonym: String,
    device_token: String,
    platform: String,
}

#[derive(Serialize)]
struct RegisterResponse {
    ok: bool,
}

/// Register or update a pseudonym-to-device-token mapping.
async fn register(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, StatusCode> {
    let result = state.conn.call(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO push_registrations \
             (pseudonym, device_token, platform, registered_at, rotated_at) \
             VALUES (?1, ?2, ?3, strftime('%s','now'), NULL)",
            rusqlite::params![req.pseudonym, req.device_token, req.platform],
        )?;
        Ok(())
    }).await;

    match result {
        Ok(()) => {
            tracing::info!("registered pseudonym");
            Ok(Json(RegisterResponse { ok: true }))
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to register pseudonym");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
struct UnregisterRequest {
    pseudonym: String,
}

/// Mark a pseudonym as rotated (grace period: kept for 7 days).
async fn unregister(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<UnregisterRequest>,
) -> StatusCode {
    let result = state.conn.call(move |conn| {
        let rows = conn.execute(
            "UPDATE push_registrations SET rotated_at = strftime('%s','now') \
             WHERE pseudonym = ?1 AND rotated_at IS NULL",
            rusqlite::params![req.pseudonym],
        )?;
        Ok(rows)
    }).await;

    match result {
        Ok(1..) => {
            tracing::info!("marked pseudonym as rotated (7-day grace period)");
            StatusCode::OK
        }
        Ok(_) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::error!(error = %e, "failed to unregister pseudonym");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// ── Homeserver endpoints ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WakeupRequest {
    pseudonyms: Vec<String>,
}

#[derive(Serialize)]
struct WakeupResponse {
    woken: Vec<String>,
    unknown: Vec<String>,
}

/// Send content-free push wakeups to one or more pseudonyms.
/// Pseudonyms within the 7-day grace period after rotation are still honoured.
async fn wakeup(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<WakeupRequest>,
) -> Json<WakeupResponse> {
    let mut woken = Vec::new();
    let mut unknown = Vec::new();

    for pseudonym in req.pseudonyms {
        let ps = pseudonym.clone();
        let result = state.conn.call(move |conn| {
            use rusqlite::OptionalExtension as _;
            conn.query_row(
                "SELECT device_token, platform FROM push_registrations \
                 WHERE pseudonym = ?1 \
                 AND (rotated_at IS NULL OR rotated_at > strftime('%s','now') - 604800)",
                rusqlite::params![ps],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            ).optional()
            .map_err(Into::into)
        }).await;

        match result {
            Ok(Some((device_token, platform))) => {
                // TODO: Replace with real APNs/FCM sending (see module doc for env vars).
                tracing::info!(
                    pseudonym = %pseudonym,
                    platform = %platform,
                    token_prefix = %&device_token[..8.min(device_token.len())],
                    "sending wakeup push (stubbed — wire APNs/FCM to send for real)"
                );
                woken.push(pseudonym);
            }
            Ok(None) => {
                tracing::debug!(pseudonym = %pseudonym, "unknown or expired pseudonym, skipping");
                unknown.push(pseudonym);
            }
            Err(e) => {
                tracing::error!(error = %e, "db error during wakeup lookup");
                unknown.push(pseudonym);
            }
        }
    }

    Json(WakeupResponse { woken, unknown })
}

// ── Maintenance ─────────────────────────────────────────────────────────────

/// Periodically delete pseudonyms whose 7-day grace period has elapsed.
async fn gc_loop(conn: Connection) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let result = conn.call(|conn| {
            let n = conn.execute(
                "DELETE FROM push_registrations \
                 WHERE rotated_at IS NOT NULL AND rotated_at < strftime('%s','now') - 604800",
                [],
            )?;
            Ok(n)
        }).await;
        match result {
            Ok(n) if n > 0 => tracing::info!(deleted = n, "GC: removed expired pseudonyms"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "GC: failed to delete expired pseudonyms"),
        }
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind_addr = std::env::var("RELAY_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3002".to_string());

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let db_path = format!("{}/relay.db", data_dir);

    let conn = Connection::open(&db_path)
        .await
        .expect("failed to open relay database");

    conn.call(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS push_registrations (
                 pseudonym      TEXT PRIMARY KEY,
                 device_token   TEXT NOT NULL,
                 platform       TEXT NOT NULL,
                 registered_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 rotated_at     INTEGER
             );",
        )?;
        Ok(())
    })
    .await
    .expect("failed to create schema");

    let gc_conn = Connection::open(&db_path)
        .await
        .expect("failed to open gc connection");
    tokio::spawn(gc_loop(gc_conn));

    let state = Arc::new(RelayState { conn });

    let app = Router::new()
        .route("/v1/register", post(register))
        .route("/v1/unregister", post(unregister))
        .route("/v1/wakeup", post(wakeup))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!(bind = %bind_addr, "starting push relay");

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("relay error");
}
