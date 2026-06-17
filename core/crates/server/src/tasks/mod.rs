//! Background cleanup tasks.
//!
//! Spawned as Tokio tasks from `main.rs`. Each runs on a fixed interval and
//! performs database maintenance that would otherwise cause unbounded growth:
//!
//! - **Message expiry** (every 60s): deletes messages past their
//!   `expires_at` timestamp.
//! - **Session token expiry** (every 5m): deletes tokens past their
//!   `expires_at` timestamp.
//! - **Prekey vacuum** (every 60s): checks the prekey pool counts for each
//!   connected device and sends a `prekey_low` WebSocket notification to any
//!   device whose count has fallen below `config.prekey_low_threshold`.
//!
//! Failures are logged but do not crash the server — the task retries on the
//! next interval.

use sqlx::PgPool;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

use crate::{db, state::{AppState, WsPush}};

/// Spawn all background tasks.
pub fn spawn_all(state: AppState) {
    let pool = state.db.clone();

    tokio::spawn(run_periodic(
        "message_expiry",
        Duration::from_secs(60),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::messages::delete_expired(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "expired messages deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "group_message_expiry",
        Duration::from_secs(60),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::group_messages::delete_expired(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "expired group messages deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "session_token_expiry",
        Duration::from_secs(300),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::sessions::delete_expired(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "expired session tokens deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "project_token_expiry",
        Duration::from_secs(300),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::project_tokens::delete_expired(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "expired project tokens deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "rate_limit_cleanup",
        Duration::from_secs(3600),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::rate_limits::delete_stale(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "stale rate limit counters deleted");
            }
            let n = db::ip_rate_limits::delete_stale(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "stale IP rate limit counters deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "challenge_expiry",
        Duration::from_secs(60),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            let n = db::challenges::delete_expired(&mut conn).await?;
            if n > 0 {
                tracing::info!(count = n, "expired auth challenges deleted");
            }
            Ok(())
        },
    ));

    tokio::spawn(run_periodic(
        "server_event_retention",
        Duration::from_secs(3600),
        pool.clone(),
        |pool| async move {
            let mut conn = pool.acquire().await?;
            // Catch-up window (docs/22): events older than this are dropped.
            const RETENTION_SECS: i64 = 30 * 86400;
            let n = db::server_events::delete_older_than(&mut conn, RETENTION_SECS).await?;
            if n > 0 {
                tracing::info!(count = n, "expired server events deleted");
            }
            Ok(())
        },
    ));


    let state_pv = state.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(60));
        let threshold = state_pv.config.prekey_low_threshold;
        loop {
            timer.tick().await;

            // Snapshot connected device PKs — don't hold the lock across DB calls.
            let device_pks: Vec<i64> =
                state_pv.ws_connections.read().await.keys().cloned().collect();
            if device_pks.is_empty() {
                continue;
            }

            let Ok(mut conn) = state_pv.db.acquire().await else {
                tracing::error!(task = "prekey_vacuum", "failed to acquire db connection");
                continue;
            };

            for device_pk in device_pks {
                // Clone the sender out of the lock guard before awaiting —
                // RwLockReadGuard is not Send and cannot be held across awaits.
                let sender = {
                    let conns = state_pv.ws_connections.read().await;
                    conns.get(&device_pk).cloned()
                };
                if let Some(sender) = sender {
                    if let Err(e) =
                        notify_if_prekeys_low(&mut conn, device_pk, threshold, &sender).await
                    {
                        tracing::error!(
                            task = "prekey_vacuum",
                            device_pk,
                            error = %e,
                            "failed to check prekey counts"
                        );
                    }
                }
            }
        }
    });
}

/// Check prekey pool counts for a device and send a `prekey_low` WebSocket
/// notification if either count is below `threshold`.
pub async fn notify_if_prekeys_low(
    conn: &mut sqlx::PgConnection,
    device_pk: i64,
    threshold: i64,
    sender: &UnboundedSender<WsPush>,
) -> Result<(), sqlx::Error> {
    let one_time = db::prekeys::one_time_count(conn, device_pk).await?;
    let kyber = db::prekeys::one_time_kyber_count(conn, device_pk).await?;
    if one_time < threshold || kyber < threshold {
        let _ = sender.send(WsPush::PrekeyLow {
            one_time_remaining: one_time,
            kyber_remaining: kyber,
        });
    }
    Ok(())
}

async fn run_periodic<F, Fut>(
    name: &'static str,
    interval: Duration,
    pool: PgPool,
    task: F,
) where
    F: Fn(PgPool) -> Fut,
    Fut: std::future::Future<Output = Result<(), sqlx::Error>>,
{
    let mut timer = tokio::time::interval(interval);
    loop {
        timer.tick().await;
        if let Err(e) = task(pool.clone()).await {
            tracing::error!(task = name, error = %e, "background task failed");
        }
    }
}
