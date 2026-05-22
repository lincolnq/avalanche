# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

All commands run from the repo root. The Rust workspace root is `core/`.

```bash
make test            # run crypto + store + types + server tests (needs Postgres running)
make test-core       # just crypto, store, types
make test-server     # just server (needs TEST_DATABASE_URL)
make test-e2e        # app-core integration tests (needs a running server)
make check           # cargo check
make clippy          # cargo clippy

# Single crate test
cd core && cargo test -p crypto
# Single test
cd core && cargo test -p crypto -- test_name

# Dev server
make dev             # run homeserver with debug logging (tower_http + server)

# Database
make db-up           # start Postgres via docker-compose
make db-down         # stop Postgres

# Mobile bindings (UniFFI → Swift + Kotlin)
make bindings        # generate bindings
make ios             # bindings + xcframework + xcodegen
```

## Key Docs

Read these first — they cover the overall design, technical approach, and backlog:

- `docs/00-design.md` — goals, architecture, threat model, first-party Project designs
- `docs/01-technical-implementation.md` — tech stack, crypto approach, repo structure, staged build plan
- `docs/02-todos-deferred.md` — deferred TODOs / backlog

Additional docs exist in `docs/` covering specific subsystems (server, projects, mobile, etc.) — numbering scheme: first digit = category (1=server, 2=projects, 3=mobile apps).

## Architecture

Monorepo with Rust core (`core/`), Swift/Kotlin mobile UIs (`mobile/`), and docs.

### Crate dependency graph

```
types ← crypto ← store ← net ← app-core
                    ↑              ↑
                  server       (UniFFI boundary to mobile)
```

- **types** — Newtypes (AccountId, DeviceId, MessageId, Timestamp). No logic.
- **crypto** — libsignal wrappers (X3DH, Double Ratchet, prekeys). **No I/O.** Defines a `Store` trait.
- **store** — SQLCipher local DB. Implements `crypto::Store`. Uses tokio-rusqlite.
- **net** — HTTP client for homeserver API (reqwest).
- **app-core** — Orchestrates crypto+store+net. UniFFI boundary for mobile.
- **server** — Axum homeserver: registration, auth, prekeys, message relay, WebSocket, DIDs.

### Critical Design Patterns

1. **crypto has no I/O.** It defines a `Store` trait; `store` implements it; `app-core` wires them together.

2. **Store is Clone (Arc-backed single connection).** Clones share one tokio-rusqlite connection, serialized on a blocking thread. This is load-bearing for libsignal's multi-`&mut` API. Do NOT replace with a connection pool.

3. **Server DB functions take `&mut PgConnection`** — callers use `pool.acquire()` for auto-commit or `pool.begin()` for transactions. Enables transaction-rollback testing.

4. **UniFFI: sync exports, global runtime.** libsignal traits return non-Send futures, so UniFFI async export doesn't work. FFI methods are sync, blocking on a `OnceLock<Runtime>`. Tests use `_async` variants.

5. **Interior mutability for FFI.** `AppCore` uses `Mutex<AppCoreInner>` since UniFFI wraps objects in `Arc`.

6. **Two error types.** `AppError` (internal, rich types) and `AppErrorFfi` (UniFFI-exported, string reasons).

7. **Default to copying Signal's approach** for crypto, protocol, UX, profiles. Only diverge where project needs require it (DIDs, federation, Projects, multi-account).

### libsignal

Pinned to commit `4c460615` (not branch main). This is a git dependency in Cargo.toml.

## Multi-terminal / Branch Workflow

- Always use `git worktree add <path> <branch>` when implementing a feature branch while the main worktree might be on a different branch — prevents uncommitted changes mixing across parallel sessions.
- Remove after pushing: `git worktree remove <path>`
- Worktrees share the same `target/` directory issues on Windows (Application Control policy); if `cargo check` fails in a worktree, fall back to checking in the main working tree.

## Adding a New Server Endpoint

1. `/new-migration <name>` — create migration file first
2. `/new-db-module <entity>` — scaffold DB layer
3. `/new-route <name>` — scaffold route + register in `routes/mod.rs`
4. Add rate limiting if the endpoint is writable or fetchable (see `middleware/rate_limit.rs`)
5. `make ci` before opening PR

Common pitfalls:
- Forgetting `.merge()` in `routes/mod.rs`
- Using `_auth` instead of `auth` if you later need `device_pk`
- `state.db.acquire().await?` not `state.db.acquire().await.unwrap()`

## UniFFI / Mobile Workflow

The full cycle for adding a new feature that involves Rust + iOS:

1. Add Rust FFI method to `core/crates/app-core/src/lib.rs` (sync, `#[uniffi::export]`)
2. `make bindings` — regenerates `mobile/ios/Generated/app_core.swift`
3. `make ios` — rebuilds XCFramework + regenerates Xcode project
4. Add to `AppCoreProtocol` in `ActnetService.swift`
5. Stub in `MockActnetService.swift`
6. Call from `AppState.swift` via `Task.detached { try core.methodName() }.value`

Use `/new-ffi-method <name>` to scaffold steps 1, 4, 5, 6 as a single command.

FFI constraints (do not violate):
- FFI exports must be **synchronous** — they block on a global tokio runtime (`OnceLock<Runtime>`)
- All FFI types must be UniFFI-compatible: `String`, `i64`, `bool`, `Vec<T>`, `Option<T>`, custom Record/Enum
- Never hold an async lock across an FFI boundary

## Error Handling Conventions (Server)

- `ServerError::Db` — propagate via `?` from `sqlx::Error` (auto via `From` impl)
- `ServerError::NotFound` — when `fetch_optional` returns `None`
- `ServerError::Unauthorized` — missing/invalid auth token; never reveal why
- `ServerError::BadRequest(msg)` — invalid input (bad base64, missing field, etc.)
- `ServerError::RateLimited` — rate limit exceeded (HTTP 429)
- `ServerError::Internal(msg)` — unexpected server-side failure; log with `tracing::error!`
- Never expose DB details or internal state to the client; only log server-side

## Contributing workflow

When implementing a TODO item from `docs/02-todos-deferred.md`:

1. Implement the feature on a dedicated branch (never directly on `main`).
2. When asked to create a PR from the feature branch to the upstream remote, **delete** the corresponding line from `docs/02-todos-deferred.md` entirely — do not use strikethrough (~~text~~), do not check it off (`- [x]`), just remove the line. Always confirm with the user which item to remove before doing so.
3. Commit the removal as part of the same PR or as a follow-up on the same branch, as the user prefers.

When merging a server-side PR that requires a corresponding client-side update (new API, changed endpoint, etc.), always add a TODO in `docs/02-todos-deferred.md` for the client work if it's not included in the same PR.
