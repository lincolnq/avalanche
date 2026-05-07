//! Background cleanup tasks.
//!
//! Spawned as Tokio tasks from `main.rs`. Each runs on a fixed interval and
//! performs database maintenance that would otherwise cause unbounded growth:
//!
//! - **Message expiry** (every 60s): deletes messages past their
//!   `expires_at` timestamp.
//! - **Session token expiry** (every 5m): deletes tokens past their
//!   `expires_at` timestamp.
//!
//! Failures are logged but do not crash the server — the task retries on the
//! next interval.

use sqlx::PgPool;
use std::time::Duration;

use crate::db;

/// Spawn all background tasks.
pub fn spawn_all(pool: PgPool) {
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
