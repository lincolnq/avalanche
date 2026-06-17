# mobile/ — iOS (and future Android) apps

## iOS

The iOS app is a Swift/SwiftUI project under `mobile/ios/Actnet/`.

### Build commands

```bash
make ios      # incremental: regenerates UniFFI bindings + XCFramework + Xcode project, then builds
make xcode    # prepare bindings + xcframework + xcodeproj for an already-open Xcode (no xcodebuild)
make bindings # regenerate UniFFI Swift/Kotlin glue only (no xcframework)
```

`make ios` does the minimum necessary work based on file dependencies — it is safe to run on every change.

### Adding a new feature (UniFFI / Mobile Workflow)

The full cycle for a feature that involves new Rust logic exposed to iOS:

1. Add the Rust FFI method to `core/crates/app-core/src/lib.rs` (sync, `#[uniffi::export]`)
2. Add to `AppCoreProtocol` in `mobile/ios/Actnet/Sources/Services/ActnetService.swift`
3. Stub in `mobile/ios/Actnet/Sources/Services/MockActnetService.swift`
4. Call from `AppState.swift` via `Task.detached { try core.methodName() }.value`
5. `make ios` to rebuild

Use `/new-ffi-method <name>` to scaffold all four steps as a single command (see `.claude/commands/new-ffi-method.md`).

### FFI constraints (do not violate)

- FFI exports must be **synchronous** — they block on a global tokio runtime (`OnceLock<Runtime>`)
- All FFI types must be UniFFI-compatible: `String`, `i64`, `bool`, `Vec<T>`, `Option<T>`, custom Record/Enum
- Never hold an async lock across an FFI boundary
- Tests use `_async` variants of FFI methods to avoid blocking the test runtime

### AppCoreProtocol pattern

`ActnetService.swift` defines `AppCoreProtocol` — the interface the app uses. All FFI methods go here. `MockActnetService.swift` provides a stub implementation for SwiftUI previews and tests. Never call `AppCore` directly from views — always go through the protocol.

### Key source files

| File | Purpose |
|---|---|
| `Sources/Services/ActnetService.swift` | `AppCoreProtocol` definition + live implementation |
| `Sources/Services/MockActnetService.swift` | Stub for previews/tests |
| `Sources/App/AppState.swift` | Top-level observable state, calls FFI via `Task.detached` |
| `Sources/App/AppState.swift` | Account list, active account, connection state |
| `project.yml` | XcodeGen project definition (source of truth for Xcode project) |
