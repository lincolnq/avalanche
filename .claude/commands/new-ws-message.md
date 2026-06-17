Add a new WebSocket frame variant named `$ARGUMENTS` to the `WsFrame` body.

## Step 1 — Proto definition (`core/proto/ws.proto`)

Read `core/proto/ws.proto` to find the current `WsFrame.body` oneof and the next available field number.

Add the new message type:
```protobuf
message $ArgumentsPascalCase {
  // fields
}
```

Add to the `WsFrame.body` oneof:
```protobuf
$ArgumentsCamelCase $ARGUMENTS = <next_field_number>;
```

Run `cd core && cargo build -p net` to regenerate the prost bindings. Fix any errors.

## Step 2 — Dispatch in ws.rs (`core/crates/net/src/ws.rs`)

Find the `match body` block in the `spawn_reader` function. Add a new arm:

```rust
Body::$ArgumentsPascalCase(msg) => {
    // Route to the appropriate channel or handle inline.
    // Patterns used by existing variants:
    //   - Send to a channel: let _ = some_tx.send(InboundSomething { ... });
    //   - Ack inline: send_frame(&outbound, &ack_frame)
    //   - Ignore silently: {}
}
```

If this message requires a new inbound channel, define the channel data type above `spawn_reader` and thread the sender/receiver through the `WsClient` struct following the pattern of `delivery_tx` or `group_delivery_tx`.

## Step 3 — Handle in connection.rs (if surfaced to app-core)

If the new frame type produces an event visible to `app-core`, open `core/crates/app-core/src/connection.rs` and handle the new inbound type in the reconnect loop. Follow the existing pattern for `InboundDelivery`, `InboundGroupDelivery`, or `InboundAccountJoined`.

## Step 4 — Verify

Run `cd core && cargo check -p net && cargo check -p app-core` and fix any errors.

Report: the proto field number assigned, the dispatch behavior, and any new channel types introduced.
