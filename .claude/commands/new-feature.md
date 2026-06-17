You are about to implement a new feature from the actnet backlog. Work through the following steps in order. Do not skip any step, and do not write any implementation code until step 6 explicitly tells you to.

## Step 1 — Select a backlog item

Read `docs/02-todos-deferred.md` in full. Display every item as a numbered list, preserving section headings.

For each item, append a platform tag based on what can be **implemented and meaningfully tested** without macOS:

- `[Any]` — fully implementable and testable on Windows/Linux: touches only server, Rust crates (`types`, `crypto`, `store`, `net`, `app-core`, `server`, `relay`), napi/Node.js, or docs. All tests run via `cargo test`, `make test`, or `make test-e2e`.
- `[macOS]` — requires macOS to test: the change lives entirely in iOS/Swift (`mobile/ios/`), or the only meaningful test is the iOS simulator.
- `[Any + macOS for full test]` — has server/Rust work doable on any platform, **and** a mobile component that requires macOS for integration testing. Server-side tests pass on any OS; full end-to-end requires macOS.

Tagging heuristic:
- If the item mentions a UI element, SwiftUI view, account switcher, scroll behavior, onboarding step, or QR code screen → `[macOS]`.
- If the item is pure server, protocol, crypto, DB schema, or Rust crate logic with no mobile surface → `[Any]`.
- If it mentions both a server-side change (new endpoint, DB column, relay message) and a mobile client consuming it → `[Any + macOS for full test]`.
- Delivery receipts, read receipts, recovery blob re-upload, sender-key recovery, and similar items that require both a server change and an iOS change → `[Any + macOS for full test]`.

After displaying the list, note which platform the user is currently on (check the session's OS context or ask if unknown) and call out which items they **cannot fully test** from their current machine.

$ARGUMENTS

If `$ARGUMENTS` is non-empty, treat it as the user's preference for which item to work on (partial name, number from the list, or verbatim line). Confirm which item you matched and ask the user to verify. If `$ARGUMENTS` is empty, ask the user to pick by number or name.

Wait for the user to confirm before continuing.

---

## Step 2 — Load design context

For the selected item, read every doc in `docs/` that is likely relevant. At minimum:
- `docs/00-design.md`
- `docs/01-technical-implementation.md`
- Any numbered doc whose title matches the feature domain (e.g. if the item mentions "recovery", read `docs/33-identity-auth-recovery.md`; if it mentions "groups", read `docs/03-groups.md`).

State in 2–3 sentences what design intent the docs establish for this feature. Identify any explicit constraints or out-of-scope notes that bear on the implementation.

---

## Step 3 — Locate the relevant code

Search the codebase to understand the current state. For each layer likely to change, read the actual source files — not just their names.

- Rust crates: `types`, `crypto`, `store`, `net`, `app-core`, `server`, `relay`
- napi layer: `core/crates/app-core-node/src/lib.rs` and `node/packages/app-core/src/index.ts` (see `node/CLAUDE.md`)
- iOS: relevant Swift files under `mobile/ios/Actnet/Sources/` (see `mobile/CLAUDE.md`)
- Search for existing partial implementations, stubs, TODO comments, or `unimplemented!()` macros.

List every file you read and summarize in one sentence what role each plays.

---

## Step 4 — Assumptions audit

Before writing the spec, state every assumption you are making — things the docs don't specify, things that depend on how existing code works, things that require a judgment call. Number each one and mark it:

- **CONFIRMED** — you found evidence in the code or docs.
- **INFERRED** — follows from design intent but not stated explicitly.
- **REQUIRES DECISION** — you cannot proceed without a human call; state the concrete options.

If any assumption is REQUIRES DECISION, stop and ask the user to resolve it before writing the spec.

---

## Step 5 — Write the spec

Present the spec below and ask for explicit approval. Do not write any implementation code until the user says "approved" or "proceed".

### Feature Spec: [feature name]

**Backlog item:** [exact line from docs/02-todos-deferred.md]

**Summary:** 1–2 sentences.

**Scope — what this PR covers:**
- [bullet list]

**Scope — explicit non-goals (follow-on work):**
- [bullet list]

**Affected layers:** [list only layers that change: `types`, `crypto`, `store`, `net`, `app-core`, `server`, `relay`, `app-core-node`, `node/ts-wrapper`, `mobile/ios`]

**Data model changes:**
New or modified DB tables (server Postgres or client SQLCipher): name, columns, migration approach.

**API changes:**
New or modified server endpoints: method, path, auth, request/response shape, error cases.
New or modified FFI exports (UniFFI or napi): signature, sync/async, error type.

**Protocol / crypto changes:**
New message types, envelope fields, or cryptographic operations. If this touches Signal Double Ratchet, Sender Keys, or zkgroup, state which primitives are used and why.

**Error handling:**
New variants needed in `AppError` / `AppErrorFfi` / `ServerError`.

**Design constraint check:**
For each Critical Design Pattern in the root `CLAUDE.md` that applies to this feature, confirm how the implementation will satisfy it (e.g. "FFI method will be sync + spawn_blocking", "Store clones shared — no new connections").

**Test plan:**

_Inline unit tests (`#[cfg(test)]`)_
List each new pure-logic function and what scenario it tests. Runs via `cargo test -p <crate>` with no external dependencies.

_Server integration tests (`core/crates/server/tests/`)_
- New DB functions: test name + `begin_tx` rollback pattern in `db_tests.rs`
- New HTTP endpoints: test name + `tower::oneshot` pattern in `http_tests.rs` or `group_tests.rs`

_Store integration tests (`core/crates/store/tests/`)_
New store operations: test using `Store::open_in_memory()`.

_app-core e2e tests (`core/crates/app-core/tests/`)_
Cross-client scenarios requiring a running server (`make test-e2e`). Note the file (`e2e_dm.rs`, `e2e_groups.rs`, or new) and what the test exercises.

_Property-based tests (proptest)_
If the feature involves a state machine, serialization format, or crypto round-trip, list the invariant to verify with `proptest!`.

_Known gaps to flag:_
- Cross-platform conformance (iOS ↔ napi producing the same result): note if `smoke.mjs` needs a new case.
- Recovery flow: if the feature touches recovery blobs or device loss, note whether the no-blob fallback path is affected.
- State machine edge cases: enumerate invalid transitions or mid-operation failures to test.

**Open questions:** remaining INFERRED assumptions for user review.

---

## Step 6 — Implement

Only after the user has approved the spec:

1. The worktree and branch should already exist from when you ran `git worktree add`. Confirm the working directory before making any changes.
2. Implement in dependency order: `types` → `crypto` → `store` → `net` → `app-core` → `server`/`relay` → `app-core-node` → `node/ts-wrapper` → `mobile/ios`.
3. After each crate change, run `cd core && cargo check -p <crate>` and fix errors before moving to the next layer.
4. Write tests described in the test plan as you implement each layer — do not defer them.
5. After all layers: run `make test` and report any failures. Do not proceed to step 7 with failing tests.

---

## Step 7 — Handoff

After `make test` passes:
1. List which tests require a running server (`make test-e2e`) and which require the napi build (`make node && npm run smoke -w @actnet/app-core`).
2. List any REQUIRES DECISION assumptions from step 4 that were deferred and should be reviewed in the diff.
3. Remind the user to run `/done` before opening a PR.
