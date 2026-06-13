Scaffold a new Axum route module named `$ARGUMENTS` and register it in the server router.

## Step 1 — Create the route file

Create `core/crates/server/src/routes/$ARGUMENTS.rs` using this pattern:

```rust
//! <One-line description of what endpoints this module provides>.
//!
//! Endpoints:
//! - `METHOD /v1/path` — <description>

use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/$ARGUMENTS", get(get_handler))
        // add more routes here
}

#[derive(Serialize)]
struct GetResponse {
    // response fields
}

async fn get_handler(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<Json<GetResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    // implementation
    Ok(Json(GetResponse {}))
}
```

Conventions:
- Use `_auth: AuthDevice` if you need auth but not `device_pk`; use `auth: AuthDevice` if you need `auth.device_pk` or `auth.account_id`
- `state.db.acquire().await?` — never `.unwrap()`
- Route paths use `/v1/` prefix
- Error handling: see `core/CLAUDE.md` for `ServerError` variants

If the endpoint is publicly accessible (no auth required), omit the `AuthDevice` extractor entirely.

If the endpoint accepts a request body, add:
```rust
use axum::extract::Json as JsonBody;
// and in the handler:
async fn post_handler(
    State(state): State<AppState>,
    auth: AuthDevice,
    JsonBody(body): JsonBody<RequestType>,
) -> Result<StatusCode, ServerError> {
```

Add rate limiting for writable or fetchable endpoints — see `middleware/rate_limit.rs` for the pattern.

## Step 2 — Register in the router

Open `core/crates/server/src/routes/mod.rs` and make two changes:

**a)** Add the module declaration (in alphabetical order):
```rust
mod $ARGUMENTS;
```

**b)** Add `.merge($ARGUMENTS::routes())` to the `router()` function (in a logical position near related routes):
```rust
.merge($ARGUMENTS::routes())
```

## Step 3 — Verify

Run `cd core && cargo check -p server` and fix any errors.

Report: the full file path, the registered endpoints with their HTTP methods and paths, and whether rate limiting was added.
