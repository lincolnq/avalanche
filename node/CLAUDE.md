# node/ — napi-rs bindings and first-party bots

This directory is an npm workspace (`node/package.json`) that builds the Node.js client library and first-party bot processes.

## Packages

| Path | Name | Purpose |
|---|---|---|
| `packages/app-core/` | `@actnet/app-core` | TypeScript wrapper over napi-rs bindings. The user-facing API. |
| `packages/adminbot/` | `@actnet/adminbot` | First-party adminbot (onboarding + `#admins` group). |

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
npm run smoke -w @actnet/app-core        # sanity-check the native addon
npm run docs -w @actnet/app-core         # generate HTML API docs → node/docs/
```

## Runtime requirements

- **Node >= 26** — enforced in `package.json`. Earlier versions lack the native `Temporal` global; the wrapper loads but any method touching a timestamp throws at runtime.
- **macOS or Linux** — build targets: `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Windows is not wired up.
- Verify `Temporal` is available: `node -e "console.log(typeof Temporal)"` should print `object`. Some `node:26-alpine` Docker images may not include it.

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

Always import from `@actnet/app-core` (the TypeScript wrapper), never from `../native/index.js` directly.

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

## Electron compatibility

The napi addon is a standard Node.js native module and works in Electron. One extra step: recompile the addon against Electron's Node.js ABI using `@electron/rebuild` before packaging, to avoid the ABI mismatch between system Node.js and Electron's bundled Node.js.

## Testing

Correctness is covered by:
- The Rust `core/crates/app-core/tests/` e2e tests (`make test-e2e`), which exercise the same `app-core` the napi layer wraps.
- `smoke.mjs` (`npm run smoke -w @actnet/app-core`), which verifies the addon loads and `Temporal` is available.

When adding a feature with a JS-side observable behavior (new event kind, new field on an existing type), add a case to `smoke.mjs` that exercises the path and asserts the TypeScript type is correct at runtime.
