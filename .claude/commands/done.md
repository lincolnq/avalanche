You are performing the post-implementation review for the current feature branch. Work through every step in order. Do not open a PR until step 6 completes without issues.

## Step 1 — Identify what was implemented

Run `git diff main...HEAD --stat`. List every changed file grouped by layer (Rust crates, napi, TypeScript, iOS/Swift, server, docs/migrations). State the feature name and the exact backlog line from `docs/02-todos-deferred.md` that this implements.

---

## Step 2 — Assumptions audit

Read the full diff (`git diff main...HEAD`). For each behavioral decision in the implementation that is not covered by a comment or spec doc, surface it as:

> **Assumption N:** [what the code assumes]. **Evidence:** [file + function]. **Risk:** [what breaks if wrong].

Check specifically for:
- `unwrap()` / `expect()` outside of test code
- Hardcoded values (timeouts, sizes, limits) that should match a protocol spec
- FFI methods taking `String` where validated bytes are expected, or vice versa
- New async operations without timeouts
- New DB columns or tables without a migration rollback path

Flag any high-risk assumption as **[NEEDS REVIEW]**. Do not proceed to step 6 without the user acknowledging all NEEDS REVIEW items.

---

## Step 3 — Test coverage check

For each item in the feature spec's test plan (from `/new-feature` step 5), verify it was implemented:

- **Inline unit tests:** `cd core && cargo test -p <crate> -- --list 2>&1` — confirm expected test names appear.
- **Server DB tests:** check for `begin_tx` tests in `core/crates/server/tests/db_tests.rs`.
- **HTTP tests:** check for `tower::oneshot` tests in `core/crates/server/tests/http_tests.rs` or `group_tests.rs`.
- **app-core e2e tests:** check for new `#[tokio::test]` functions in `core/crates/app-core/tests/`.
- **proptest:** if the spec called for property-based tests, verify they exist.

For any test from the spec that is missing, write it now before continuing. After writing missing tests, run `make test` and confirm it passes.

If the spec flagged known gaps (cross-platform conformance, recovery flow, state machine edge cases), verify the PR either addresses them or adds a `// TODO(tests):` comment with a new entry in `docs/02-todos-deferred.md`.

---

## Step 4 — Architecture constraint check

Review the diff against each Critical Design Pattern in the root `CLAUDE.md`:

1. **crypto has no I/O.** Does any new code in `crypto/` call `store` or `net`? If yes, it is a violation.
2. **Store is Clone (Arc-backed).** Does any new code replace Store with a pool or hold a direct connection? If yes, flag it.
3. **Server DB functions take `&mut PgConnection`.** Do any new DB functions accept `&PgPool`? If yes, they can't use the `begin_tx` test pattern.
4. **FFI methods are synchronous.** Do any new `#[uniffi::export]` methods return a `Future`?
5. **napi wrappers use spawn_blocking.** Do any new `#[napi]` async fn bodies call `app-core` synchronously without `spawn_blocking`?
6. **AppCore uses Mutex<AppCoreInner>.** Does any new code access `AppCoreInner` fields without the Mutex?
7. **Two error types.** Do new internal errors use `AppError`? Do errors crossing FFI use `AppErrorFfi` with a `From<AppError>` impl?

Stop and fix any violation before proceeding.

---

## Step 5 — TODO deletion

Read `docs/02-todos-deferred.md`. Identify the exact line(s) this feature implements.

Show the user the line(s) and ask: "Should I delete [quoted line] from docs/02-todos-deferred.md?" Wait for confirmation, then delete exactly those lines — no strikethrough, no checkbox, no restructuring of surrounding sections.

Run `git diff docs/02-todos-deferred.md` and show the user the diff for a final check.

If this PR merges a server-side change that requires a matching client-side update (new endpoint, changed response shape), add a new TODO to `docs/02-todos-deferred.md` under the appropriate section now.

---

## Step 6 — PR draft

Run `make test && make check && make clippy` and confirm all pass. If anything fails, fix and re-run before continuing.

Draft the PR using `.github/pull_request_template.md`. Fill in every applicable section:

**Title:** Short, under 70 characters.

**Summary:** What was implemented and why — reference the backlog item by its exact wording.

**Layer checklist:** Check off every applicable item in the template's server / mobile-FFI / crypto sections.

**Test evidence:** Which test layers were exercised and whether they pass. For e2e tests requiring a running server, note whether they were run locally or deferred.

**Assumptions for reviewer:** The numbered assumption list from step 2. Put any NEEDS REVIEW items first, bolded.

**TODO deletion:** Confirm the backlog line was removed.

Then run `gh pr create --title "<title>" --body "<body>"` with the drafted content.
