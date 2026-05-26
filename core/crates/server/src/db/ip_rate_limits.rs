use sqlx::{PgConnection, Row};

/// Atomically increment the counter for (ip, action) in the current fixed
/// window and return whether the request is within the limit.
///
/// The window is aligned to epoch time in chunks of `window_secs` seconds.
/// Returns `true` if allowed, `false` if the limit has been reached.
///
/// `ACTNET_DISABLE_IP_RATE_LIMITS=1` bypasses the check entirely (returns
/// `Ok(true)` without touching the DB). Intended for the dev server and
/// e2e tests, where a developer iterating locally would otherwise burn
/// through the registration quota in a handful of runs. Production never
/// sets this.
pub async fn check_and_increment(
    conn: &mut PgConnection,
    ip: &str,
    action: &str,
    limit: i32,
    window_secs: i64,
) -> Result<bool, sqlx::Error> {
    if std::env::var("ACTNET_DISABLE_IP_RATE_LIMITS").ok().as_deref() == Some("1") {
        return Ok(true);
    }

    let row = sqlx::query(
        "INSERT INTO ip_rate_limit_counters (ip, action, window_start, count)
         VALUES (
             $1, $2,
             to_timestamp(floor(extract(epoch from now()) / $3) * $3),
             1
         )
         ON CONFLICT (ip, action, window_start)
         DO UPDATE SET count = ip_rate_limit_counters.count + 1
         RETURNING count",
    )
    .bind(ip)
    .bind(action)
    .bind(window_secs as f64)
    .fetch_one(&mut *conn)
    .await?;

    let count: i32 = row.get("count");
    Ok(count <= limit)
}

/// Delete counters whose window started more than one hour ago.
pub async fn delete_stale(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM ip_rate_limit_counters WHERE window_start < now() - interval '1 hour'",
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}
