Add a new `ContentMessage` body type named `$ARGUMENTS`. This touches four layers: proto definition, Rust dispatch, napi layer, and iOS.

## Step 1 — Proto definition (`core/proto/content.proto`)

Read `core/proto/content.proto` first to find the next available field number in the `ContentMessage.body` oneof (currently reserved up to 14; last used is 9).

Add the new message type and wire it into the oneof:

```protobuf
message $ArgumentsPascalCase {
  // fields
  string field_name = 1;
}
```

Add to the `ContentMessage.body` oneof:
```protobuf
$ArgumentsCamelCase $ARGUMENTS = <next_field_number>;
```

After editing, run `cd core && cargo build -p types` to regenerate the Rust proto bindings (prost runs at build time). Fix any build errors.

## Step 2 — Dispatch in messaging.rs (`core/crates/app-core/src/messaging.rs`)

Find the `match content.body` block in the `process_decrypted` function. Add a new match arm:

```rust
Some(Body::$ArgumentsPascalCase(msg)) => {
    // Handle the new message type.
    // Common patterns:
    //   - Persist to store: inner.store.save_something(...).await?
    //   - Update state: inner.something = msg.field
    //   - Emit an event: return Ok(Some(DecryptedMessage { ... }))
    //   - Silent control message: return Ok(None)
}
```

Add the `None` arm at the end if not already present:
```rust
None => {
    tracing::debug!("ignoring ContentMessage with no body");
}
```

## Step 3 — Sending (if this type is sent by clients)

If clients need to send this message type, add a builder in `messaging.rs` and expose it via an FFI method. Follow the `/new-ffi-method` command for the FFI scaffolding.

## Step 4 — napi layer

If the new type is surfaced as an event to Node.js bots, add:
1. A `*Js` struct with `#[napi(object)]` in `core/crates/app-core-node/src/lib.rs`
2. A new `IncomingEvent` kind variant in `node/packages/app-core/src/index.ts`

Follow the existing `IncomingEvent` discriminated union pattern.

## Step 5 — iOS (if surfaced to mobile UI)

If the type produces a visible event in the iOS app:
1. Add a case to the relevant Swift enum in `mobile/ios/Actnet/Sources/Models/`
2. Handle it in the appropriate view

## Step 6 — Verify

Run `cd core && cargo check -p app-core` and fix any errors. Run `make node` if the napi layer was touched.

Report: the proto field number assigned, the dispatch behavior, and whether it produces a user-visible event.
