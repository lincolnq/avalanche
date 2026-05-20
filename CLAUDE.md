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

Requires: `protobuf` (`brew install protobuf` on macOS).

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

## Contributing workflow

When implementing a TODO item from `docs/02-todos-deferred.md`:

1. Implement the feature on a dedicated branch (never directly on `main`).
2. When asked to create a PR from the feature branch to the upstream remote, remove the corresponding item from `docs/02-todos-deferred.md` — but **always confirm with the user which item to remove before doing so**.
3. Commit the removal as part of the same PR or as a follow-up on the same branch, as the user prefers.
