//! One-time prekey pool replenishment.
//!
//! Clients upload a fixed batch of one-time prekeys at registration and the
//! homeserver consumes one per first-contact session. Without replenishment a
//! heavily-contacted account drains its pools to zero, after which every new
//! sender falls back to the signed / last-resort keys.
//!
//! This module tops the pools back up. It's driven from `connection.rs` by two
//! triggers: a proactive check right after each (re)connect (covers draining
//! while offline / a missed push), and the server's `PrekeyLow` WebSocket push
//! (covers draining while connected). Both call [`replenish_if_low`].
//!
//! IDs come from the persistent monotonic allocator in `store`
//! (`allocate_prekey_ids`) so a refilled key never reuses the id of a consumed
//! one — reuse could let a stale PreKey/Kyber message match a fresh key.

use crate::{error::AppError, AppCore};

/// Refill a pool once the server reports it below this many keys. Matches the
/// server's default `prekey_low_threshold` (`server/src/tasks/mod.rs`).
const LOW_THRESHOLD: i64 = 10;

/// Bring a low pool back up to this many keys — the same count uploaded at
/// registration (`lib.rs` `create_inner`).
const REFILL_TARGET: i64 = 20;

/// Top up the one-time EC and Kyber prekey pools if the server reports either
/// running low. Best-effort and idempotent: safe to call on every connect and
/// on every `PrekeyLow` push. Errors are logged, never propagated — a failed
/// refill must not disturb the reconnect loop.
pub(crate) async fn replenish_if_low(core: &AppCore) {
    if let Err(e) = try_replenish(core).await {
        tracing::warn!("[prekeys] replenishment failed: {e}");
    }
}

async fn try_replenish(core: &AppCore) -> Result<(), AppError> {
    let inner = core.inner.lock().await;
    let status = inner.client.prekey_status().await?;

    if status.one_time_remaining < LOW_THRESHOLD {
        let count = (REFILL_TARGET - status.one_time_remaining) as u32;
        let start = inner.store.allocate_prekey_ids("one_time", count).await?;
        let generated = crypto::prekeys::generate_one_time_prekeys(start, count as usize)?;
        inner
            .store
            .save_one_time_prekeys(
                &generated
                    .iter()
                    .map(|k| (k.wire.id, k.record.clone()))
                    .collect::<Vec<_>>(),
            )
            .await?;
        inner
            .client
            .upload_prekeys(&net::types::UploadPrekeysRequest {
                signed_prekey: None,
                one_time_prekeys: Some(
                    generated
                        .iter()
                        .map(|k| (k.wire.id as i32, k.wire.public_key.clone()))
                        .collect(),
                ),
                kyber_prekey: None,
                one_time_kyber_prekeys: None,
            })
            .await?;
        tracing::info!(
            "[prekeys] refilled {count} one-time EC prekeys (ids {}..{})",
            start,
            start + count
        );
    }

    if status.kyber_remaining < LOW_THRESHOLD {
        let count = (REFILL_TARGET - status.kyber_remaining) as u32;
        // Kyber prekeys are signed by the identity key.
        let identity = inner.store.load_identity().await?.ok_or(AppError::NoAccount)?;
        let start = inner.store.allocate_prekey_ids("kyber", count).await?;
        let generated = (start..start + count)
            .map(|id| crypto::prekeys::generate_kyber_prekey(&identity, id))
            .collect::<Result<Vec<_>, _>>()?;
        inner
            .store
            .save_kyber_prekeys(
                &generated
                    .iter()
                    .map(|k| (k.wire.id, k.record.clone()))
                    .collect::<Vec<_>>(),
            )
            .await?;
        inner
            .client
            .upload_prekeys(&net::types::UploadPrekeysRequest {
                signed_prekey: None,
                one_time_prekeys: None,
                kyber_prekey: None,
                one_time_kyber_prekeys: Some(
                    generated
                        .iter()
                        .map(|k| {
                            (k.wire.id as i32, k.wire.public_key.clone(), k.wire.signature.clone())
                        })
                        .collect(),
                ),
            })
            .await?;
        tracing::info!(
            "[prekeys] refilled {count} one-time Kyber prekeys (ids {}..{})",
            start,
            start + count
        );
    }

    Ok(())
}
