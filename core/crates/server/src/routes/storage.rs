//! Storage service endpoints (docs/05-device-data-sync.md §5):
//!
//! - `GET  /v1/storage/items?since={cursor}&limit={n}` — authenticated delta pull
//! - `PUT  /v1/storage/items` — authenticated batch write, per-item CAS
//! - `GET  /v1/storage/snapshot` — authenticated whole-store snapshot fetch
//! - `PUT  /v1/storage/snapshot` — authenticated snapshot push, LWW on version
//!
//! All are scoped to the caller's own account: a device reads and writes only
//! its own identity store. Records are opaque ciphertext (`record_id` is an HMAC
//! of type+key, see §4); the server enforces only byte/count quotas (§10), never
//! anything type-aware.
//!
//! The `/items` pair is the live, single-authoritative read/write path; the
//! `/snapshot` pair is the passive-backup side-channel (§7) — one whole-store
//! blob per account, pushed one-way to the identity's non-authoritative accounts
//! and read only on recovery.

use axum::{
    extract::{DefaultBodyLimit, Query, State},
    routing::get,
    Json, Router,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

// ── Quotas (§10) ─────────────────────────────────────────────────────────────
//
// The governing limit is snapshot/recovery cost, not raw storage. Large content
// never enters the store (it goes to the media path; the store holds references),
// so records are intrinsically small. Limits are byte/count only — any semantic
// ("max N contacts") cap lives client-side.
const MAX_RECORD_BYTES: usize = 8 * 1024; // per-record ciphertext: stops a record becoming a file
const MAX_TOTAL_BYTES: i64 = 8 * 1024 * 1024; // total live bytes / account: binds first
const MAX_RECORD_COUNT: i64 = 25_000; // secondary guard

// Defensive bounds on request shape.
const DEFAULT_PULL_LIMIT: i64 = 500;
const MAX_PULL_LIMIT: i64 = 1000;
const MAX_WRITES_PER_REQUEST: usize = 500;

// A snapshot is the whole store re-encoded into one blob (§7), so it is bounded
// by the per-account total (`MAX_TOTAL_BYTES`, 8 MB) plus envelope overhead.
// Allow comfortable headroom above that.
const MAX_SNAPSHOT_BYTES: usize = 12 * 1024 * 1024;
// The snapshot blob is base64-encoded inside a JSON body, which inflates it by
// ~4/3. Raise the request body limit above axum's 2 MB default so a legitimate
// snapshot push isn't rejected before the handler's own cap applies.
const SNAPSHOT_BODY_LIMIT: usize = 20 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/storage/items", get(pull_items).put(put_items))
        .route(
            "/v1/storage/snapshot",
            get(get_snapshot).put(put_snapshot)
                .layer(DefaultBodyLimit::max(SNAPSHOT_BODY_LIMIT)),
        )
}

fn decode_b64(s: &str) -> Result<Vec<u8>, ServerError> {
    BASE64_STANDARD
        .decode(s)
        .map_err(|_| ServerError::BadRequest("invalid base64".into()))
}

/// Resolve the authenticated device to its account id.
async fn account_for(
    conn: &mut sqlx::PgConnection,
    auth: &AuthDevice,
) -> Result<i64, ServerError> {
    let device = db::devices::find_by_pk(conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("device not found for session".into()))?;
    Ok(device.account_id)
}

// ── GET /v1/storage/items ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PullQuery {
    #[serde(default)]
    since: i64,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct PullItem {
    record_id: String, // base64
    version: i64,
    seq: i64,
    deleted: bool,
    ciphertext: String, // base64; empty for tombstones
}

#[derive(Serialize)]
struct PullResponse {
    items: Vec<PullItem>,
    next_cursor: i64,
    has_more: bool,
}

async fn pull_items(
    State(state): State<AppState>,
    auth: AuthDevice,
    Query(q): Query<PullQuery>,
) -> Result<Json<PullResponse>, ServerError> {
    let limit = q
        .limit
        .unwrap_or(DEFAULT_PULL_LIMIT)
        .clamp(1, MAX_PULL_LIMIT);

    let mut conn = state.db.acquire().await?;
    let account_id = account_for(&mut conn, &auth).await?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        account_id,
        crate::middleware::rate_limit::ACTION_STORAGE_PULL,
        crate::middleware::rate_limit::LIMIT_STORAGE_PULL,
        crate::middleware::rate_limit::WINDOW_STORAGE_PULL,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let rows = db::storage::pull(&mut conn, account_id, q.since, limit).await?;
    let has_more = rows.len() as i64 == limit;
    let next_cursor = rows.last().map(|r| r.seq).unwrap_or(q.since);
    let items = rows
        .into_iter()
        .map(|r| PullItem {
            record_id: BASE64_STANDARD.encode(&r.record_id),
            version: r.version,
            seq: r.seq,
            deleted: r.deleted,
            ciphertext: BASE64_STANDARD.encode(&r.ciphertext),
        })
        .collect();

    Ok(Json(PullResponse {
        items,
        next_cursor,
        has_more,
    }))
}

// ── PUT /v1/storage/items ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WriteReq {
    record_id: String, // base64
    expected_version: i64,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    ciphertext: String, // base64; empty/absent for tombstones
}

#[derive(Serialize)]
struct AppliedItem {
    record_id: String, // base64
    version: i64,
    seq: i64,
}

#[derive(Serialize)]
struct ConflictItem {
    record_id: String, // base64
    current_version: i64,
}

#[derive(Deserialize)]
struct PutRequest {
    writes: Vec<WriteReq>,
}

#[derive(Serialize)]
struct PutResponse {
    applied: Vec<AppliedItem>,
    conflicts: Vec<ConflictItem>,
}

/// A decoded, validated write ready to apply.
struct DecodedWrite {
    record_id: Vec<u8>,
    record_id_b64: String,
    expected_version: i64,
    deleted: bool,
    ciphertext: Vec<u8>,
}

async fn put_items(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<PutRequest>,
) -> Result<Json<PutResponse>, ServerError> {
    if req.writes.len() > MAX_WRITES_PER_REQUEST {
        return Err(ServerError::BadRequest("too many writes in one request".into()));
    }

    // Decode + per-record byte cap up front, before touching the DB.
    let mut writes = Vec::with_capacity(req.writes.len());
    let mut incoming_live_bytes: i64 = 0;
    let mut incoming_live_count: i64 = 0;
    for w in &req.writes {
        let record_id = decode_b64(&w.record_id)?;
        if record_id.is_empty() {
            return Err(ServerError::BadRequest("empty record_id".into()));
        }
        // Stored verbatim even for tombstones: the client seals a small routing
        // header (type tag + logical key, no payload) into a deleted record's
        // ciphertext so a pulling device can route the deletion. The server
        // stays type-blind — it's opaque bytes either way. Tombstone bytes are
        // excluded from the live byte/count quota (account_usage filters them).
        let ciphertext = decode_b64(&w.ciphertext)?;
        if ciphertext.len() > MAX_RECORD_BYTES {
            return Err(ServerError::BadRequest("record exceeds size limit".into()));
        }
        if !w.deleted {
            incoming_live_bytes += ciphertext.len() as i64;
            incoming_live_count += 1;
        }
        writes.push(DecodedWrite {
            record_id,
            record_id_b64: w.record_id.clone(),
            expected_version: w.expected_version,
            deleted: w.deleted,
            ciphertext,
        });
    }

    let mut tx = state.db.begin().await?;
    let account_id = account_for(&mut tx, &auth).await?;

    if !db::rate_limits::check_and_increment(
        &mut tx,
        account_id,
        crate::middleware::rate_limit::ACTION_STORAGE_PUSH,
        crate::middleware::rate_limit::LIMIT_STORAGE_PUSH,
        crate::middleware::rate_limit::WINDOW_STORAGE_PUSH,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    // Account-wide quota (§10). Conservative: counts every non-deleted write as a
    // new live record/byte-add, which over-counts in-place updates. Records are
    // intrinsically tiny, so erring toward rejection near the cap is acceptable.
    let (cur_bytes, cur_count) = db::storage::account_usage(&mut tx, account_id).await?;
    if cur_bytes + incoming_live_bytes > MAX_TOTAL_BYTES
        || cur_count + incoming_live_count > MAX_RECORD_COUNT
    {
        return Err(ServerError::BadRequest("storage quota exceeded".into()));
    }

    let mut applied = Vec::new();
    let mut conflicts = Vec::new();
    for w in &writes {
        match db::storage::put_item(
            &mut tx,
            account_id,
            &w.record_id,
            w.expected_version,
            w.deleted,
            &w.ciphertext,
        )
        .await?
        {
            db::storage::PutOutcome::Applied { version, seq } => applied.push(AppliedItem {
                record_id: w.record_id_b64.clone(),
                version,
                seq,
            }),
            db::storage::PutOutcome::Conflict { current_version } => {
                conflicts.push(ConflictItem {
                    record_id: w.record_id_b64.clone(),
                    current_version,
                })
            }
        }
    }

    tx.commit().await?;

    // Fast-sync nudge (docs/05 §8): if anything was applied, tell this account's
    // *other* connected devices to delta-pull promptly. Best-effort and purely a
    // latency optimization — correctness is the cursor pull, so a dropped nudge
    // (offline device, send race) is harmless. Never nudge the writer itself.
    if let Some(high_seq) = applied.iter().map(|a| a.seq).max() {
        if let Ok(mut conn) = state.db.acquire().await {
            if let Ok(pks) = db::devices::pks_for_account(&mut conn, account_id).await {
                let conns = state.ws_connections.read().await;
                for pk in pks {
                    if pk == auth.device_pk {
                        continue;
                    }
                    if let Some(tx) = conns.get(&pk) {
                        let _ = tx.send(crate::state::WsPush::StorageChanged { high_seq });
                    }
                }
            }
        }
    }

    Ok(Json(PutResponse { applied, conflicts }))
}

// ── GET /v1/storage/snapshot ─────────────────────────────────────────────────

#[derive(Serialize)]
struct SnapshotResponse {
    snapshot_version: i64,
    blob: String, // base64
}

async fn get_snapshot(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<Json<SnapshotResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let account_id = account_for(&mut conn, &auth).await?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        account_id,
        crate::middleware::rate_limit::ACTION_STORAGE_SNAPSHOT_GET,
        crate::middleware::rate_limit::LIMIT_STORAGE_SNAPSHOT_GET,
        crate::middleware::rate_limit::WINDOW_STORAGE_SNAPSHOT_GET,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let snap = db::storage::get_snapshot(&mut conn, account_id)
        .await?
        .ok_or(ServerError::NotFound)?;
    Ok(Json(SnapshotResponse {
        snapshot_version: snap.snapshot_version,
        blob: BASE64_STANDARD.encode(&snap.blob),
    }))
}

// ── PUT /v1/storage/snapshot ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct PutSnapshotRequest {
    snapshot_version: i64,
    blob: String, // base64
}

#[derive(Serialize)]
struct PutSnapshotResponse {
    /// True iff this push won LWW and is now the stored snapshot.
    stored: bool,
    /// The snapshot_version held after this request (the incoming one if it
    /// won, otherwise the newer one already on file).
    snapshot_version: i64,
}

async fn put_snapshot(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<PutSnapshotRequest>,
) -> Result<Json<PutSnapshotResponse>, ServerError> {
    if req.snapshot_version < 0 {
        return Err(ServerError::BadRequest("negative snapshot_version".into()));
    }
    let blob = decode_b64(&req.blob)?;
    if blob.is_empty() {
        return Err(ServerError::BadRequest("empty snapshot blob".into()));
    }
    if blob.len() > MAX_SNAPSHOT_BYTES {
        return Err(ServerError::BadRequest("snapshot exceeds size limit".into()));
    }

    let mut conn = state.db.acquire().await?;
    let account_id = account_for(&mut conn, &auth).await?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        account_id,
        crate::middleware::rate_limit::ACTION_STORAGE_SNAPSHOT_PUT,
        crate::middleware::rate_limit::LIMIT_STORAGE_SNAPSHOT_PUT,
        crate::middleware::rate_limit::WINDOW_STORAGE_SNAPSHOT_PUT,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let outcome =
        db::storage::put_snapshot(&mut conn, account_id, req.snapshot_version, &blob).await?;
    let resp = match outcome {
        db::storage::SnapshotOutcome::Stored { snapshot_version } => PutSnapshotResponse {
            stored: true,
            snapshot_version,
        },
        db::storage::SnapshotOutcome::Stale { current_version } => PutSnapshotResponse {
            stored: false,
            snapshot_version: current_version,
        },
    };
    Ok(Json(resp))
}
