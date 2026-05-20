use sqlx::{PgConnection, Row};

/// Atomically increments the counter for (account_id, action) in the current
/// fixed window and returns whether the request is within the limit.
///
/// The window is aligned to epoch time in chunks of `window_secs` seconds.
/// Returns `true` if allowed, `false` if the limit has been reached.
pub async fn check_and_increment(
    conn: &mut PgConnection,
    account_id: i64,
    action: &str,
    limit: i32,
    window_secs: i64,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO rate_limit_counters (account_id, action, window_start, count)
         VALUES (
             $1, $2,
             to_timestamp(floor(extract(epoch from now()) / $3) * $3),
             1
         )
         ON CONFLICT (account_id, action, window_start)
         DO UPDATE SET count = rate_limit_counters.count + 1
         RETURNING count",
    )
    .bind(account_id)
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
        "DELETE FROM rate_limit_counters WHERE window_start < now() - interval '1 hour'",
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}
