# node/ — napi-rs bindings and first-party bots

This directory is an npm workspace (`node/package.json`) that builds the Node.js client library and first-party bot processes.

## Packages

| Path | Name | Purpose |
|---|---|---|
| `packages/app-core/` | `@theavalanche/app-core` | TypeScript wrapper over napi-rs bindings. The user-facing API. |
| `packages/adminbot/` | `@theavalanche/adminbot` | First-party adminbot (onboarding + `#admins` group). |

## Build commands

From the repo root:

```bash
make node          # release build: cargo cdylib → napi glue → tsc
make node-debug    # debug build (much faster, use during development)
make adminbot      # build + run adminbot against ADMINBOT_SERVER_URL
```

From `node/` directly:

```bash
npm install                              # install once
npm run build                            # equivalent to make node
npm run build:debug                      # equivalent to make node-debug
npm run smoke -w @theavalanche/app-core        # sanity-check the native addon
npm run docs -w @theavalanche/app-core         # generate HTML API docs → node/docs/
```

## Runtime requirements

- **Node >= 26** — enforced in `package.json`. Earlier versions lack the native `Temporal` global; the wrapper loads but any method touching a timestamp throws at runtime.
- **macOS or Linux** — build targets: `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Windows is not wired up.
- Verify `Temporal` is available: `node -e "console.log(typeof Temporal)"` should print `object`. Some `node:26-alpine` Docker images may not include it.

## Publishing `@theavalanche/app-core` to npm

`@theavalanche/app-core` is published to the public npm registry on **stable** git tags
(a `vX.Y.Z` with no prerelease suffix), by the `napi-build` + `publish-npm` jobs in
`.github/workflows/release.yml`. The npm version is the tag minus its `v`, matching
the repo's one-tag-one-version scheme. It ships as **split platform packages**:
`@theavalanche/app-core` plus four `@theavalanche/app-core-<platform>` sub-packages
(linux x64/arm64, macOS x64/arm64) declared as `optionalDependencies`; the
napi-generated `native/index.js` loader resolves the right one at runtime. There is
**no `postinstall` build step**, so npm v12's opt-in-lifecycle-scripts default is a
non-issue.

Steady-state auth is **OIDC trusted publishing** (no secret): the workflow's
`id-token: write` grant lets npm mint a short-lived credential, and provenance is
emitted automatically. This is the replacement for the deprecated 2FA-bypass
automation tokens (which lose publish rights ~Jan 2027).

**Why the first publish is different.** A trusted publisher attaches to an
*existing* package (via the npmjs package-settings page), and the main package
pins all four `@theavalanche/app-core-<platform>` sub-packages at the same version
— so the first publish must build all four triples and create all five names at
once. A dev Mac can only build its own triple (`darwin-arm64`), and OIDC can't be
used yet (nothing to attach a trusted publisher to). So for the first publish we
build the binaries in CI, download them, and publish from a logged-in machine — no
token is ever stored in CI.

### One-time setup (manual, operator)

1. Create the free **`@theavalanche`** org on npmjs.com (public packages are free)
   and `npm login` locally as a member (2FA ready).
2. **Build the four binaries in CI:** Actions → Release → *Run workflow* (the
   `workflow_dispatch` trigger). This runs ONLY the `napi-build` job — the
   release/publish jobs are gated to tag pushes — producing four `napi-<triple>`
   artifacts. Download them into one dir: `gh run download <run-id> --dir /tmp/napi`.
3. **Publish all five packages from your machine:** from `node/packages/app-core`,
   `./bootstrap-publish.sh 0.4.0 /tmp/napi` (version WITHOUT the leading `v`). It
   stages the binaries + loader glue, builds the TS wrapper, and publishes the four
   sub-packages then the main package (npm prompts for your 2FA code).
4. On npmjs.com, for **each** of the five now-existing packages
   (`@theavalanche/app-core` + the four `@theavalanche/app-core-<platform>`), add a
   **Trusted Publisher**: GitHub repo `lincolnq/avalanche`, workflow `release.yml`.
5. Done. Every later stable `vX.Y.Z` tag publishes from CI via OIDC — no secret,
   provenance attested. Prerelease tags (`v0.5.0-rc.1`) build the GitHub Release
   but skip npm.

(The `publish-npm` job also honours an `NPM_TOKEN` repo secret if one is set, as an
escape hatch, but the bootstrap above needs no token and steady-state uses OIDC.)

**Local validation (optional, no publish):** from `node/packages/app-core` run
`make node` then `npm publish --dry-run` to inspect the main tarball. Always run
napi commands **from the package dir** — the `napi` config (triples, addon name)
lives in `packages/app-core/package.json`; running from `node/` makes napi fall
back to wrong defaults (`index.*` name, win32/x64 triples).

## Crate layout

```
core/crates/app-core-node/   ← Rust cdylib (native addon source)
  src/lib.rs                 ← #[napi] wrappers around app-core
  build.rs                   ← napi-build (generates napi glue)
  Cargo.toml                 ← [lib] crate-type = ["cdylib"]

node/packages/app-core/
  src/index.ts               ← TypeScript wrapper (what callers import)
  native/                    ← napi-rs generated glue + .node addon (git-ignored)
  dist/                      ← tsc output (git-ignored)
```

Always import from `@theavalanche/app-core` (the TypeScript wrapper), never from `../native/index.js` directly.

## Architectural decisions

### spawn_blocking everywhere

`app-core` is synchronous (UniFFI forces sync FFI — see `mobile/CLAUDE.md`). Every `#[napi]` async wrapper calls `tokio::task::spawn_blocking` so the napi reactor never stalls. Do NOT call `app-core` methods synchronously inline in a napi async fn.

### JS type conversions

The raw napi-generated types use primitives. The TypeScript wrapper converts at the boundary:

| Native (napi) type | TypeScript wrapper type | Note |
|---|---|---|
| `number` (epoch ms) | `Temporal.Instant` | `instantFromMs` / `instantToMs` |
| `number` (0–3) | `DeliveryStatus` string union | `deliveryFromNum` / `deliveryToNum` |
| `number` (0–1) | `GroupRole` string union | `roleFromNum` / `roleToNum` |
| `Buffer` | `Uint8Array` | `asU8` / `asBuf` (zero-copy) |

### DID types

- `did:plc:...` — human accounts registered against the PLC directory.
- `did:local:...` — bot accounts. Server assigns the suffix at registration; adminbot uses the reserved suffix `adminbot` → `did:local:adminbot`.

### events() vs nextEvents()

`events()` is an async generator and is the standard receive path. `nextEvents()` is the lower-level batch poll — use it only when you need explicit batch-processing semantics. Both are **single-consumer**: running two concurrent readers on the same `AppCore` instance produces undefined behavior.

## TypeScript conventions

- Files use `"type": "module"` with `moduleResolution: nodenext`. Use `.js` extensions in import paths even for `.ts` source files.
- No `any`. For discriminated unions, add a `default: throw new Error("malformed event")` branch to handle unknown future kinds gracefully.
- Timestamps: always `Temporal.Instant` in user-facing code. Convert at the napi boundary only.
- Errors from the native layer arrive as plain `Error` objects whose `.message` is the Rust `AppErrorFfi` string reason.

## Adminbot

Adminbot persistent state lives under `ADMINBOT_STATE_DIR` (default: `node/adminbot-state/`):
- `store.db` — SQLCipher database managed by app-core.
- `state.json` — adminbot's own bookkeeping (`adminsGroupId`, `invitedInitialAdmins`).

| Environment variable | Default | Purpose |
|---|---|---|
| `ADMINBOT_SERVER_URL` | `http://localhost:3000` | Homeserver to register against |
| `ADMINBOT_STATE_DIR` | `node/adminbot-state` | SQLCipher DB + state.json location |
| `ADMINBOT_INITIAL_ADMINS` | (empty) | Comma-separated DIDs to invite at bootstrap |
| `ADMINBOT_DB_KEY` | (empty) | SQLCipher passphrase for the store |

## Adding a new method to the API

Follow all four steps — skipping any one breaks the layering:

1. **`core/crates/app-core/src/lib.rs`** — add the sync method with `#[uniffi::export]`. This is the canonical surface shared by mobile and Node.
2. **`core/crates/app-core-node/src/lib.rs`** — add a `#[napi]` async wrapper using `spawn_blocking`. For new return types, define a `*Js` struct with `#[napi(object)]` and a `From<FfiFoo> for FooJs` impl.
3. **`node/packages/app-core/src/index.ts`** — add converter functions following the `fooFromNative` / `fooToNative` naming pattern; expose the method on the `AppCore` class with JSDoc.
4. **`make node`** to verify the whole chain compiles.

The napi surface must mirror the UniFFI surface exactly — bots and the iOS app must be able to do identical things.

## Node.js compatibility

The napi addon is a standard Node.js native module — it works in any Node.js context (bots, scripts, CLI tools). The desktop app uses Tauri (not napi/Electron); the napi layer is for bots and server-side tooling only.

## Testing

Correctness is covered by:
- The Rust `core/crates/app-core/tests/` e2e tests (`make test-e2e`), which exercise the same `app-core` the napi layer wraps.
- `smoke.mjs` (`npm run smoke -w @theavalanche/app-core`), which verifies the addon loads and `Temporal` is available.

When adding a feature with a JS-side observable behavior (new event kind, new field on an existing type), add a case to `smoke.mjs` that exercises the path and asserts the TypeScript type is correct at runtime.
