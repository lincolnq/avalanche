# core/ — Rust workspace

The Rust workspace root is `core/`. See the root `CLAUDE.md` for the crate dependency graph and critical design patterns.

## Adding a New Server Endpoint

1. `/new-migration <name>` — create migration file (see `.claude/commands/new-migration.md`)
2. `/new-db-module <entity>` — scaffold DB layer (see `.claude/commands/new-db-module.md`)
3. `/new-route <name>` — scaffold route + register in `routes/mod.rs` (see `.claude/commands/new-route.md`)
4. Add rate limiting if the endpoint is writable or fetchable (see `middleware/rate_limit.rs`)
5. `make ci` before opening PR

Common pitfalls:
- Forgetting `.merge()` in `routes/mod.rs`
- Using `_auth` instead of `auth` if you later need `device_pk`
- `state.db.acquire().await?` not `state.db.acquire().await.unwrap()`

## Error Handling Conventions (Server)

- `ServerError::Db` — propagate via `?` from `sqlx::Error` (auto via `From` impl)
- `ServerError::NotFound` — when `fetch_optional` returns `None`
- `ServerError::Unauthorized` — missing/invalid auth token; never reveal why
- `ServerError::BadRequest(msg)` — invalid input (bad base64, missing field, etc.)
- `ServerError::RateLimited` — rate limit exceeded (HTTP 429)
- `ServerError::Internal(msg)` — unexpected server-side failure; log with `tracing::error!`
- Never expose DB details or internal state to the client; only log server-side

## Test Patterns

### Unit tests
Inline `#[cfg(test)]` modules at the bottom of source files. No external dependencies — use these for pure logic (crypto, encoding, state transitions).

### Server integration tests (`crates/server/tests/`)
- **DB tests** (`db_tests.rs`): use `begin_tx()` for transaction-rollback isolation — each test gets a clean slate without truncating tables.
- **HTTP tests** (`http_tests.rs`, `group_tests.rs`): use `test_state()` + `tower::oneshot` to call handlers directly without a running server.
- `ensure_setup()` via `OnceCell` applies migrations once across all tests in a process.

### app-core e2e tests (`crates/app-core/tests/`)
Require a running homeserver at `SERVER_URL` (default: `http://localhost:3000`). Run via `make test-e2e`. Each test creates fresh accounts to avoid interference. Use `test_store()` for in-memory SQLite stores and `only_from()` to filter out adminbot welcome DMs.

### test-utils crate
`crates/test-utils/` provides `TestClient` — a helper with identity keys and a store — for session-level crypto tests.
