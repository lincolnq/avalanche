Add a new group action named `$ARGUMENTS`. This spans the wire type, client-side state machine, server handler, FFI export, and napi layer.

## Step 1 — Wire type (`core/crates/net/src/groups.rs`)

Read `core/crates/net/src/groups.rs` to understand `GroupActionsWire`. Add a new field:

```rust
// In GroupActionsWire:
#[serde(default, skip_serializing_if = "Option::is_none")]  // or Vec::is_empty
pub $ARGUMENTS: Option<$ArgumentsWire>,  // or Vec<$ArgumentsWire>
```

Define the wire struct above `GroupActionsWire`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct $ArgumentsWire {
    // fields — use base64-encoded bytes for encrypted data (String), plain types otherwise
}
```

## Step 2 — Client-side action (`core/crates/app-core/src/groups.rs`)

Add an async function that uses the `submit_actions` helper. This is the canonical pattern — read `submit_actions` in `groups.rs` before writing this:

```rust
pub async fn $ARGUMENTS(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    // action-specific params
) -> Result<(), AppError> {
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, _group_key| {
            // 1. Validate: check preconditions against current state
            //    e.g. if member not found: return Err(AppError::Protocol("...".into()))
            
            // 2. Apply optimistic update to `state` in-place
            //    e.g. state.members.retain(|m| ...)
            
            // 3. Return the wire action
            Ok(GroupActionsWire {
                $ARGUMENTS: Some($ArgumentsWire { /* fields */ }),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}
```

The `submit_actions` helper handles:
- Loading cached group state
- Applying the closure to compute optimistic state + wire action
- Submitting to server with revision number
- On 409: refreshing state from server and retrying once
- Updating local cache on success

## Step 3 — FFI export (`core/crates/app-core/src/lib.rs`)

Add a sync FFI wrapper inside `#[uniffi::export] impl AppCore`:

```rust
pub fn $ARGUMENTS(
    &self,
    group_id: String,
    // action-specific params
) -> Result<(), AppErrorFfi> {
    ffi_runtime().block_on(async {
        let inner = self.inner.lock().await;
        groups::$ARGUMENTS(
            &inner.store,
            &inner.client,
            &inner.did,
            &group_id,
            // params
        ).await?;
        Ok::<_, AppError>(())
    }).map_err(AppErrorFfi::from)
}
```

## Step 4 — Server handler

The server receives group actions via the existing group actions endpoint. Read `core/crates/server/src/routes/groups.rs` to find where `GroupActionsWire` is processed server-side. Add handling for the new action field in the server's action processor.

## Step 5 — napi wrapper

Follow `/new-ffi-method` step 2 to add the napi async wrapper in `core/crates/app-core-node/src/lib.rs` and step 3 to expose it in `node/packages/app-core/src/index.ts`.

## Step 6 — iOS Swift

Follow `/new-ffi-method` steps 4 to add the method to `AppCoreProtocol`, `MockActnetService`, and call it from `AppState.swift`.

## Step 7 — Verify

Run `cd core && cargo check -p net && cargo check -p app-core && cargo check -p server`. Fix all errors before moving to step 5.

Report: the wire struct fields, the optimistic state mutation, and the server-side behavior.
