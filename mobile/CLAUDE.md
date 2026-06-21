# mobile/ — iOS and Android apps

## Platform Parity Rule

**Any feature added or changed on iOS must be implemented on Android in the same
session. Any feature added or changed on Android must be implemented on iOS.**

iOS is the reference implementation. Android must match it feature-for-feature.
When in doubt about behavior, check the iOS source.

The same rule applies across all three platforms — see the root parity rule in
`CLAUDE.md` and `desktop/CLAUDE.md` for Desktop.

Use `docs/60-android-implementation.md` as the parity tracking document — update
the `[ ]` / `[x]` checkboxes as each component is completed.

---

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
| `project.yml` | XcodeGen project definition (source of truth for Xcode project) |

---

## Android

The Android app is a Kotlin/Jetpack Compose project under `mobile/android/`.

### Build commands

```bash
make android            # full build: bindings + native libs, then Gradle builds the debug APK
make android-bindings   # prep only: regenerate Kotlin UniFFI glue + cross-compile
                        # libapp_core.so per ABI into app/src/main/jniLibs/ (no Gradle).
                        # The Android analog of `make xcode`.
```

`make android` does the minimum necessary work based on file dependencies — the
Rust cross-compile only reruns when Rust sources change. Gradle needs a JDK 17+;
the Makefile falls back to Android Studio's bundled JBR if `JAVA_HOME` is unset.

The Rust core is consumed directly (UniFFI-generated Kotlin in `mobile/android/Generated/`
as a source dir + `libapp_core.so` in `jniLibs/`, loaded via JNA), not packaged as
an AAR. Both `Generated/` and `jniLibs/` are gitignored build artifacts.

### FFI constraints (same as iOS, different syntax)

- All UniFFI calls from Kotlin must use `withContext(Dispatchers.IO) { core.method() }`
- The WebSocket loop runs in `viewModelScope` coroutines, cancelled when ViewModel is cleared
- Min SDK: 26 (Android 8.0)

The SQLCipher DB key is `"dev-placeholder-key"` until Android Keystore integration lands.

See `docs/60-android-implementation.md` for the full parity map and implementation phases.

---

## Visual Reference: Screenshots

`docs/screenshots/` contains iOS simulator screenshots organized by screen name
(e.g. `splash.png`, `chats-list.png`, `conversation.png`). When implementing a
screen on Android or Desktop, use the matching screenshot as a visual reference
if it exists. If it doesn't exist, derive the layout from the iOS source alone —
screenshots are optional, not required.

Screenshots are only capturable on macOS with the iOS simulator. Contributors on
Windows or Linux skip this step entirely.

---

## Adding a New Screen (checklist)

Before closing any branch that adds or changes mobile UI:

- [ ] iOS SwiftUI view created/updated in `mobile/ios/`
- [ ] Android Compose screen created/updated in `mobile/android/`
- [ ] AppState (iOS) and AppViewModel (Android) updated consistently
- [ ] New model fields added to both `.swift` and `.kt` data classes
- [ ] `docs/60-android-implementation.md` parity table updated
- [ ] *(macOS only)* Screenshot taken and saved to `docs/screenshots/<screen-name>.png`

## Adding a New FFI Method (checklist)

1. Add Rust method to `core/crates/app-core/src/lib.rs` (`#[uniffi::export]`, sync)
2. `make bindings` — regenerates Swift + Kotlin glue
3. Add to `AppCoreProtocol` in `ActnetService.swift` and stub in `MockActnetService.swift`
4. Add to `ActnetService` interface in Kotlin and stub in `MockActnetService.kt`
5. Call from `AppState.swift` via `Task.detached`
6. Call from `AppViewModel.kt` via `withContext(Dispatchers.IO)`
7. Update Desktop simultaneously (see `desktop/CLAUDE.md`)
