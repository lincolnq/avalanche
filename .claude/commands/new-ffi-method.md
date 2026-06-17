Scaffold a new FFI method named `$ARGUMENTS` that is exposed to both the iOS app (via UniFFI) and the Node.js layer (via napi-rs).

Work through the four layers in order. After each layer, verify the change compiles before moving to the next.

---

## Step 1 — Rust: app-core (`core/crates/app-core/src/lib.rs`)

Add a sync method inside the `#[uniffi::export] impl AppCore` block. Follow this exact pattern:

```rust
pub fn method_name(
    &self,
    // parameters here — UniFFI-compatible types only:
    // String, i64, bool, Vec<u8>, Option<T>, custom Record/Enum
) -> Result<ReturnType, AppErrorFfi> {
    ffi_runtime().block_on(async {
        let ws = self.ws.lock().expect("ws mutex poisoned").clone();
        let mut inner = self.inner.lock().await;
        // implementation
        Ok::<_, AppError>(result)
    }).map_err(AppErrorFfi::from)
}
```

If the method only needs a read lock on `inner`, use `self.inner.lock().await` without `mut`. If it does not need the WebSocket, omit the `ws` line.

If the return type requires a new record type, define it in `lib.rs` with `#[derive(uniffi::Record)]` before the `impl` block.

Run `cd core && cargo check -p app-core` and fix any errors before continuing.

---

## Step 2 — Rust: napi wrapper (`core/crates/app-core-node/src/lib.rs`)

Add a corresponding `#[napi]` async method to the `impl AppCore` block. Follow this pattern:

```rust
#[napi]
pub async fn method_name(
    &self,
    // parameters — napi-compatible types: String, i64, bool, Buffer, Vec<T>
) -> Result<ReturnTypeJs, napi::Error> {
    let core = self.inner.clone();
    tokio::task::spawn_blocking(move || {
        core.method_name(/* params */)
            .map(ReturnTypeJs::from)
            .map_err(to_napi)
    })
    .await
    .map_err(join_err)?
}
```

If the return type is a new struct, define it with `#[napi(object)]` and implement `From<ReturnTypeFfi> for ReturnTypeJs`.

Run `cd core && cargo check -p app-core-node` and fix any errors before continuing.

---

## Step 3 — TypeScript wrapper (`node/packages/app-core/src/index.ts`)

Add the method to the `AppCore` class. Follow the existing conversion patterns:

```typescript
async methodName(/* params */): Promise<ReturnType> {
  const raw = await this.#native.methodName(/* converted params */);
  return returnTypeFromNative(raw);
}
```

Add any new converter functions (`fooFromNative` / `fooToNative`) near the top of the file alongside the existing converters. Export any new public types.

---

## Step 4 — iOS Swift

**a) `AppCoreProtocol` in `mobile/ios/Actnet/Sources/Services/ActnetService.swift`**

Add the method signature to the protocol:

```swift
func methodName(param: ParamType) throws -> ReturnType
```

**b) `MockActnetService.swift`**

Add a stub implementation to `MockAppCore` that returns a sensible default:

```swift
func methodName(param: ParamType) throws -> ReturnType {
    // return a mock value
}
```

**c) Call site in `AppState.swift`** (if needed — only if this method has a UI call site)

```swift
let result = try await Task.detached {
    try core.methodName(param: value)
}.value
```

---

## Step 5 — Verify

Run `make node` to confirm the full Rust → napi → TypeScript chain compiles. If the method has a UI call site, run `make ios` (requires macOS).

Report: the method signature on each layer, and any new types introduced.
