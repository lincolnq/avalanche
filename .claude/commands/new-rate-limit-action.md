Add a new rate limit action for `$ARGUMENTS`.

## Step 1 — Add constants to rate_limit.rs

Open `core/crates/server/src/middleware/rate_limit.rs` and add three constants following the existing naming pattern:

```rust
pub const ACTION_$ARGUMENTS_UPPER: &str = "$ARGUMENTS_lower";
pub const LIMIT_$ARGUMENTS_UPPER: i32 = <N>;   // max requests per window
pub const WINDOW_$ARGUMENTS_UPPER: i64 = <N>;  // window size in seconds
```

Choose limit and window based on the action type:
- High-frequency reads (e.g. fetch messages): `LIMIT=100, WINDOW=60`
- Writes/mutations (e.g. send message): `LIMIT=100, WINDOW=60`
- Expensive operations (e.g. upload prekeys): `LIMIT=10, WINDOW=3600`
- Account-level operations (e.g. update profile): `LIMIT=20, WINDOW=3600`
- Creation operations (e.g. create group): `LIMIT=5, WINDOW=3600`

For per-IP limits (registration, unauthenticated endpoints), use the IP rate limit pattern — search for `ACTION_REGISTER` in `rate_limit.rs` to see the difference.

## Step 2 — Apply in the handler

In the route handler that needs limiting, add the rate limit check after acquiring a DB connection:

```rust
use crate::middleware::rate_limit::{ACTION_$ARGUMENTS_UPPER, LIMIT_$ARGUMENTS_UPPER, WINDOW_$ARGUMENTS_UPPER};

async fn my_handler(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<..., ServerError> {
    let mut conn = state.db.acquire().await?;
    
    // Rate limit check — place before any expensive work
    crate::db::rate_limits::check_and_increment(
        &mut conn,
        auth.account_id,
        ACTION_$ARGUMENTS_UPPER,
        LIMIT_$ARGUMENTS_UPPER,
        WINDOW_$ARGUMENTS_UPPER,
    )
    .await?;
    
    // rest of handler
}
```

## Step 3 — Verify

Run `cd core && cargo check -p server` and fix any errors.

Report: the constants added and which handler(s) they were applied to.
