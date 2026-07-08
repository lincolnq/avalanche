# core/ — Rust workspace

The Rust workspace root is `core/`. See the root `CLAUDE.md` for the crate dependency graph and critical design patterns.

## Code Navigation & Refactors

For finding usages, call sites, and definitions — prefer the **LSP tool** over grep or throwaway scripts. rust-analyzer is type- and scope-aware, so it catches aliased/re-exported usages grep misses and produces no false hits from comments or strings, and these operations are read-only:

- `findReferences` — every usage of a trait/method/type (e.g. censusing `Store` usages before a refactor)
- `incomingCalls` / `outgoingCalls` — the call graph around a function
- `goToDefinition`, `goToImplementation`, `workspaceSymbol` — locate symbols

Use `Grep` only for non-symbol patterns (string literals, comments, macros the LSP can't resolve). Reserve `Bash` for things the LSP genuinely can't answer — don't write code to census usages.

Note: rust-analyzer may under-report until indexing finishes. Run `make check` first to warm it up before a large `findReferences` sweep. The LSP applies no edits — actual changes still go through Edit/Write.

## Concurrency: never hold the `inner` lock across a network call

`AppCore` wraps its mutable state in `Mutex<AppCoreInner>` (a tokio async mutex).
That lock is **per-account** and serializes everything that takes it. The rule:

> **Never hold `inner` across a network `.await`.** A homeserver that is slow or
> unreachable can make a network call block for up to the connect/request timeout
> (~15–30s, see `net::Client::new`). Anything else that needs `inner` — including,
> historically, the reads that paint the UI — is stuck behind it for that whole
> time. This is exactly the bug that made the app hang on launch when a second
> account's server was offline (the reconnect task held `inner` across the lazy
> auth handshake).

Store `.await`s under the lock are fine — `store::DeviceStore` is a fast, local,
self-serializing connection. It's **network** awaits that must stay off-lock.

**The pattern** (see `connection.rs::try_connect_ws` for the canonical example):
clone the handle you need out from under the guard, drop the guard, *then* do I/O.

```rust
let client = { let inner = core.inner.lock().await; inner.client.clone() };
client.ensure_authenticated().await?;   // network, off-lock
```

**Lock-free handles.** `AppCore` exposes `store`, `client`, and `did` directly
(cheap clones set in `AppCore::build`; `DeviceStore` shares one connection,
`net::Client` shares auth + pool). Read-only and idempotent-write FFI paths use
these and **must not** take `inner` at all — e.g. `load_conversations`,
`cached_display_names`, `get_account_info`, `register_push_token`,
`try_replenish`. When you add such a method, reach for `self.store` / `self.client`,
not `self.inner.lock()`.

**The deliberate exception** is the crypto send/group paths (`send_dm`,
`send_to_target`, group sends, `accept_invite`, recovery) — methods on
`AppCoreInner` in `messaging.rs` / `groups.rs`. They hold `inner` to serialize
Double-Ratchet / Sender-Key state mutation, and a few still fetch prekey bundles
or POST under the lock. That is a known, bounded carve-out, **not** a license to
add more network under the lock: if you touch these, prefer lifting the network
out (fetch → lock+encrypt → send) over adding a new in-lock fetch.

**Review checkpoint.** Any new `inner.lock().await` scope that contains an
`inner.client.<...>().await` (or calls a helper that hits the network while the
guard is live) must be justified against this rule before merge.

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
