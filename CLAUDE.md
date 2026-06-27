# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

All commands run from the repo root. The Rust workspace root is `core/`.

```bash
# Most-used
make ios             # build the iOS app for the simulator (incremental)
make xcode           # prepare bindings + xcframework + xcodeproj for an
                     # already-open Xcode (no xcodebuild)
make android         # build the Android debug APK (incremental)
make android-bindings # Android prep only: Kotlin UniFFI glue + native libs, no Gradle
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
- **Android** — Kotlin/Jetpack Compose, UniFFI bindings. See `docs/60-android-implementation.md`.
- **Desktop** — Tauri/Solid/TypeScript, Tauri commands. See `docs/61-desktop-implementation.md`.

**Any feature added or changed on one platform must be implemented on all three
in the same session.** iOS is the reference — when behavior is ambiguous, check
the iOS source. See `mobile/CLAUDE.md` and `desktop/CLAUDE.md` for platform-specific
workflows and per-platform checklists.

## DM / Group Parity Rule

**DMs and groups are two targets of the *same* conversation, not two features.
Any change to a message/content flow for DMs must be made for groups in the same
session, and vice versa** — sending, attachments, link previews, reactions,
edits, deletes, read state, rendering. Treat this with the same discipline as the
cross-platform parity rule above: audit both before considering the change done.

This matters because the split is easy to miss: app-core already unifies the two
behind a `MessageTarget` (`send_to_target`, `send_message(target:)`), but the
mobile layer still has *separate* `sendMessage` (DM) and `sendGroupMessage`
(group) methods in `AppState`/`AppViewModel`, and `ConversationView` branches on
`conversation.isGroup`. So "add it to the send path" is two edits, and wiring
only the one in front of you (usually DM) silently leaves groups behind. When you
touch one branch, grep for its sibling and change both. (The deeper fix is to
collapse the mobile send onto the unified `send_message(target:)` path so the
split can't exist — do that when it's in scope.)

## Feature conventions

- **Group/admin actions surface a system message by default.** Any group
  membership or metadata change — join, leave, invite, remove, role change
  (admin grant/revoke), title/description/expiry/policy change — must appear as
  a system line in the conversation timeline. app-core already persists these as
  `message_history` rows with `kind > 0` (the `kind_code` offset; docs/03 §3.6)
  and emits `IncomingEvent::GroupMetadataChanged`, so every UI must (a) render
  `kind > 0` rows as centered system text (resolving actor/target DIDs to display
  names via the structured `metadata`, falling back to the row `body`), and
  (b) refresh the affected group's timeline on `GroupMetadataChanged`. When you
  add a new group action, it surfaces a system message **unless the design doc
  explicitly says it should be silent** (e.g. DM `TimerChange` is deliberately
  silent — `app-core/src/messaging.rs`). This applies on all three platforms.

## Subsystem docs

Each subsystem has its own CLAUDE.md with workflow and conventions specific to that layer:

- `desktop/CLAUDE.md` — Tauri app workflow, Tauri commands bridge, Desktop parity rule
- `mobile/CLAUDE.md` — iOS + Android workflows, FFI constraints, parity rule
- `core/CLAUDE.md` — server endpoint workflow, error handling conventions

## Behavior Rules

- **Cite sources.** When making a claim about the codebase (what a module does, whether something exists, how a pattern works), cite the file and line range (e.g. `path/to/file.rs:42-57`). Do not state things as fact without having read them. Do not rely solely on CLAUDE.md descriptions — read the actual source and cite it.
- **Spec before code.** Before writing implementation code for a new feature, write a spec covering what changes, what files are touched, what assumptions are being made, and a test plan. Use `/new-feature` to run the full workflow. Wait for explicit approval before implementing.
- **Assumptions audit.** After completing an implementation, list what was assumed but not explicitly verified.
- **Strong config defaults, not mandatory vars.** A new config value should have a default that just works for the base case — the documented/canonical deployment — so nothing has to be set for a standard install. Derive the default from how we actually deploy (e.g. the `avalanche.service` systemd `WorkingDirectory`/`ReadWritePaths`, the `install.sh` paths), have the deploy ensure it (create the dir, set ownership), and let other deployments take their cue and override via env. Dev overrides in `dev.py` / the `Makefile` dev targets. Reserve "refuse to start unless set" for things with no safe default at all (real secrets) — not for paths/sizes/limits where a sensible default exists.
- **Never make a breaking interface/contract change without explicit review.** A "contract" is anything other code or other people depend on: a shared/public API or endpoint shape, the wire/protocol format, the FFI surface, a cross-platform behavior contract (iOS/Android/Desktop parity), the project interface (e.g. how a project webview receives its auth token), DB schema/migrations, or any persisted format. If a change would alter one of these, **stop and surface it** — do not just implement it, even if it looks like an improvement (including security hardening). Explain the change and its blast radius and wait for the maintainer's explicit go-ahead; the maintainer reviews it with the project owner before it proceeds. When the change is genuinely warranted, make it consistently across every consumer (e.g. all three platforms) in the same change, not on one platform unilaterally. Prefer an additive/backward-compatible path; if you can't avoid a break, say so explicitly.

## UniFFI / Mobile Workflow

The full cycle for adding a new feature that involves Rust + all platforms:

1. Add Rust FFI method to `core/crates/app-core/src/lib.rs` (sync, `#[uniffi::export]`)
2. `make bindings` — regenerates Swift + Kotlin UniFFI glue
3. `make xcode` — rebuilds XCFramework + regenerates Xcode project
4. **iOS:** add to `AppCoreProtocol` in `ActnetService.swift`, stub in `MockActnetService.swift`, call from `AppState.swift` via `Task.detached { try core.methodName() }.value`
5. **Android:** add to `ActnetService.kt` interface, stub in `MockActnetService.kt`, call from `AppViewModel.kt` via `withContext(Dispatchers.IO)`
6. **Desktop:** add Tauri command in `desktop/src-tauri/src/lib.rs`, add typed wrapper in `DevServerActnetService.ts`, stub in `MockActnetService.ts`

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

## Local Claude Hooks (optional, Windows/PowerShell)

The committed `.claude/settings.json` is kept minimal (`{}`) so it works on all platforms.
Platform-specific hooks belong in `.claude/settings.local.json` (gitignored — create it
yourself; Claude Code will not create it automatically). On Windows, three PostToolUse
hooks are useful:

- **Bash / `gh pr create`** — reminds you to delete the matching line from `docs/02-todos-deferred.md`
- **Edit** and **Write on platform files** — reminds you to implement the equivalent on the other two platforms (iOS / Android / Desktop)

To set them up: copy `.claude/settings.local.json` from a teammate who has them configured.

## Contributing workflow

The PR template at `.github/pull_request_template.md` is auto-populated by GitHub when you open a PR. It includes layer-specific checklists (server, mobile/FFI, crypto) — fill in every applicable section before requesting review.

When implementing a TODO item from `docs/02-todos-deferred.md`:

1. Implement the feature on a dedicated branch (never directly on `main`).
2. When asked to create a PR from the feature branch to the upstream remote, **delete** the corresponding line from `docs/02-todos-deferred.md` entirely — do not use strikethrough (~~text~~), do not check it off (`- [x]`), just remove the line. Always confirm with the user which item to remove before doing so.
3. Commit the removal as part of the same PR or as a follow-up on the same branch, as the user prefers.

When merging a server-side PR that requires a corresponding client-side update (new API, changed endpoint, etc.), always add a TODO in `docs/02-todos-deferred.md` for the client work if it's not included in the same PR.
