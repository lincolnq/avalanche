Scaffold a new Axum request extractor (middleware) named `$ARGUMENTS`.

## Step 1 — Create the middleware file

Create `core/crates/server/src/middleware/$ARGUMENTS.rs`. Follow the `FromRequestParts` pattern used by `auth.rs` and `client_ip.rs`:

```rust
//! `$ARGUMENTS` extractor — <one-line description of what this extracts/validates>.

use axum::{
    extract::FromRequestParts,
    http::request::Parts,
};
use axum::extract::FromRef;

use crate::{error::ServerError, state::AppState};

/// Extracted from a request by <describe trigger: header, query param, etc.>.
pub struct $ArgumentsPascalCase {
    // extracted fields
}

impl<S> FromRequestParts<S> for $ArgumentsPascalCase
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ServerError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let _app_state = AppState::from_ref(state);
        // extraction logic
        // return Err(ServerError::Unauthorized) to reject
        // return Err(ServerError::BadRequest("reason".into())) for malformed input
        Ok($ArgumentsPascalCase {
            // fields
        })
    }
}
```

If the extractor needs a DB lookup, add:
```rust
let mut conn = _app_state.db.acquire().await.map_err(ServerError::Db)?;
```

If the extractor only reads headers (no DB, no state), omit the `AppState: FromRef<S>` bound and use `impl<S: Send + Sync> FromRequestParts<S>` instead.

## Step 2 — Register the module

Open `core/crates/server/src/middleware/mod.rs` and add:
```rust
pub mod $ARGUMENTS;
```

## Step 3 — Use in a route handler

In any route handler, add the extractor as a parameter:
```rust
async fn my_handler(
    State(state): State<AppState>,
    extracted: $ArgumentsPascalCase,
    // ... other extractors
) -> Result<..., ServerError> {
```

Axum runs extractors in order — place auth extractors before domain extractors.

## Step 4 — Verify

Run `cd core && cargo check -p server` and fix any errors.

Report: the extractor struct fields and the rejection condition(s).
