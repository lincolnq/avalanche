# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

All commands run from the repo root. The Rust workspace root is `core/`.

```bash
# Most-used
make ios             # build the iOS app for the simulator (incremental)
make xcode           # prepare bindings + xcframework + xcodeproj for an
                     # already-open Xcode (no xcodebuild)
make dev-all         # run homeserver + testbot + relay together (preferred)
make test            # run crypto + store + types + server tests (needs Postgres)
make test-e2e        # app-core integration tests (needs a running server)

# Database
make db-up           # start Postgres via docker-compose
make db-down         # stop Postgres

# Less common
make test-core       # just crypto, store, types
make test-server     # just server (needs TEST_DATABASE_URL)
make bindings        # regenerate UniFFI Swift/Kotlin glue only (no xcframework)
make dev             # run homeserver alone with debug logging
make check           # cargo check
make clippy          # cargo clippy

# Single crate test
cd core && cargo test -p crypto
# Single test
cd core && cargo test -p crypto -- test_name
```

The iOS targets are file-dependency driven, so `make ios` does the right
minimum work. See the header of `Makefile` for the build chain.

## Key Docs

**Before any design, architecture, or cross-subsystem work, read `docs/DIGEST.md` first.** It is a ~12K-token compressed index of every design doc — decisions, their rationale, and rejected alternatives, with pointers back to the source doc for detail — so you can hold the whole design in context at once. (For a trivial, localized change you can skip it.) Source docs remain authoritative; regenerate the digest if the docs change materially.

For further detail, open the relevant source doc — the digest cites them inline (e.g. `(03 §3.9)`):

- `docs/00-design.md` — goals, architecture, threat model, first-party Project designs
- `docs/01-technical-implementation.md` — tech stack, crypto approach, repo structure, staged build plan
- `docs/02-todos-deferred.md` — deferred TODOs / backlog

Additional docs exist in `docs/` covering specific subsystems (server, projects, mobile, etc.) — numbering scheme: first digit = category (0=core design, 1=server & protocol, 2=projects, 3=messaging & conversation UX, 4=deploy/infra, 5=identity/accounts/contacts). See the documentation map at the top of `docs/00-design.md` for the full index.

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

## Cross-Platform Parity Rule

The app has three UI platforms sharing one Rust core:

- **iOS** — Swift/SwiftUI, UniFFI bindings. Reference implementation.
- **Android** — Kotlin/Jetpack Compose, UniFFI bindings. See `docs/android-implementation.md`.
- **Desktop** — Electron/React/TypeScript, napi-rs bindings. See `docs/desktop-implementation.md`.

**Any feature added or changed on one platform must be implemented on all three
in the same session.** iOS is the reference — when behavior is ambiguous, check
the iOS source. See `mobile/CLAUDE.md` and `desktop/CLAUDE.md` for platform-specific
workflows and per-platform checklists.

## Subsystem docs

Each subsystem has its own CLAUDE.md with workflow and conventions specific to that layer:

- `desktop/CLAUDE.md` — Electron app workflow, napi-rs bridge, Desktop parity rule
- `mobile/CLAUDE.md` — iOS + Android workflows, FFI constraints, parity rule
- `core/CLAUDE.md` — server endpoint workflow, error handling conventions

## Behavior Rules

- **Cite sources.** When making a claim about the codebase (what a module does, whether something exists, how a pattern works), cite the file and line range (e.g. `path/to/file.rs:42-57`). Do not state things as fact without having read them. Do not rely solely on CLAUDE.md descriptions — read the actual source and cite it.
- **Spec before code.** Before writing implementation code for a new feature, write a spec covering what changes, what files are touched, what assumptions are being made, and a test plan. Use `/new-feature` to run the full workflow. Wait for explicit approval before implementing.
- **Assumptions audit.** After completing an implementation, list what was assumed but not explicitly verified.

## UniFFI / Mobile Workflow

The full cycle for adding a new feature that involves Rust + all platforms:

1. Add Rust FFI method to `core/crates/app-core/src/lib.rs` (sync, `#[uniffi::export]`)
2. `make bindings` — regenerates Swift + Kotlin UniFFI glue
3. `make ios` — rebuilds XCFramework + regenerates Xcode project
4. **iOS:** add to `AppCoreProtocol` in `ActnetService.swift`, stub in `MockActnetService.swift`, call from `AppState.swift` via `Task.detached { try core.methodName() }.value`
5. **Android:** add to `ActnetService.kt` interface, stub in `MockActnetService.kt`, call from `AppViewModel.kt` via `withContext(Dispatchers.IO)`
6. **Desktop:** add IPC handler in `desktop/src/main/ipc.ts`, add typed wrapper in `DevServerActnetService.ts`, stub in `MockActnetService.ts`

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

The PR template at `.github/pull_request_template.md` is auto-populated by GitHub when you open a PR. It includes layer-specific checklists (server, mobile/FFI, crypto) — fill in every applicable section before requesting review.

When implementing a TODO item from `docs/02-todos-deferred.md`:

1. Implement the feature on a dedicated branch (never directly on `main`).
2. When asked to create a PR from the feature branch to the upstream remote, **delete** the corresponding line from `docs/02-todos-deferred.md` entirely — do not use strikethrough (~~text~~), do not check it off (`- [x]`), just remove the line. Always confirm with the user which item to remove before doing so.
3. Commit the removal as part of the same PR or as a follow-up on the same branch, as the user prefers.

When merging a server-side PR that requires a corresponding client-side update (new API, changed endpoint, etc.), always add a TODO in `docs/02-todos-deferred.md` for the client work if it's not included in the same PR.
