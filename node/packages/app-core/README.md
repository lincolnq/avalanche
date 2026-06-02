# @actnet/app-core

Node.js bindings for [actnet](https://github.com/civitech/actnet)'s `app-core` — the Rust client library that implements account creation, end-to-end encrypted messaging, and group flows on top of libsignal + SQLCipher.

This package is a thin TypeScript layer over a `napi-rs` cdylib. Bots and server-side automation talk to a homeserver through this API the same way the iOS app does.

## Install

This crate isn't on npm yet; build from the actnet checkout:

```bash
# from repo root
make node          # release build (~30s clean)
make node-debug    # faster turnaround during development

# or, inside node/
npm install
npm run build
```

Outputs:

- `node/native/app-core.<triple>.node` — the native addon
- `node/native/index.{js,d.ts}` — auto-generated napi binding (raw types)
- `node/dist/index.{js,d.ts}` — the user-facing wrapper (use this)

## Runtime requirements

- **Node ≥ 26** with a native `Temporal` global. Standard nodejs.org binaries include it. Some distro images (notably `node:26-alpine` without the Rust toolchain at build time) ship without `Temporal`; the wrapper itself loads fine, but any method that takes or returns a `Temporal.Instant` will throw.
- **macOS / Linux** — the build script targets `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Windows isn't wired up.

## Quick start: a bot that echoes DMs

```ts
import { initLogging, AppCore } from "@actnet/app-core";

initLogging("info");

const core = await AppCore.createAccount(
  "https://homeserver.example",
  "/var/lib/echobot/store.db",
  "",                // SQLCipher passphrase (optional)
  new Uint8Array(0),     // no recovery blob for a bot
  "EchoBot",
);

console.log("registered as", core.did());

for await (const e of core.events()) {
  if (e.kind !== "message") continue;
  await core.sendDm(e.message.senderDid, `you said: ${e.message.body}`);
}
```

## Conventions

- **Timestamps** — every `*At` field is a `Temporal.Instant`. Use `Temporal.Now.instant()` to produce them; `Number(i.epochMilliseconds)` to drop down to a raw number when you must.
- **Messages are strings** — `sendDm` / `sendGroupMessage` take a `body: string`; received messages expose `body: string`. UTF-8 conversion happens at the wrapper boundary. The raw `plaintext: Uint8Array` is also available on `DecryptedMessage` for cases where a future application encodes binary payloads.
- **Other bytes** — non-message byte fields (master keys, profile keys, recovery keys, pseudonyms) are `Uint8Array`. Inputs accept any `Uint8Array`; `Buffer` passes through zero-copy as it's already a subclass.
- **Enums** — `deliveryStatus` is `"sending" | "sent" | "delivered" | "read"`; group `role` is `"member" | "admin"`. Conversions happen at the boundary.
- **Async** — every Rust call runs on the napi libuv threadpool, so awaiting them never blocks the JS event loop. `events()` (and the underlying `nextEvents()`) is single-consumer; run from exactly one async loop.
- **Errors** — Rust errors come through as plain `Error` objects with the `AppErrorFfi` reason in `.message`.

## Generating HTML documentation

```bash
cd node
npm run docs       # writes ./docs (open docs/index.html)
```

The browsable reference is generated from the JSDoc on `src/index.ts` via [TypeDoc](https://typedoc.org).

## What lives where

| Path                                  | Description                                  |
| ------------------------------------- | -------------------------------------------- |
| `src/index.ts`                        | The TypeScript wrapper (what users import).  |
| `native/`                             | Generated napi-rs glue + the `.node` addon.  |
| `dist/`                               | tsc output (the published `main`/`types`).   |
| `../core/crates/app-core-node/`       | The Rust crate that produces the addon.      |
| `../core/crates/app-core/`            | The underlying client library (Rust).        |
