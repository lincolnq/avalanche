# mobile/ — iOS and Android apps

## Platform parity

iOS is the reference implementation; Android mirrors it screen for screen. The parity rule
itself (and its bug-fix exception) is in the root `CLAUDE.md`. Track feature parity in
**`docs/62-feature-parity.md`** — the single parity matrix. `docs/60-android-implementation.md`
holds Android implementation notes, not a tracker.

The end-to-end workflow for a new FFI method (Rust → bindings → iOS → Android → Desktop) is
in the root `CLAUDE.md` ("UniFFI / Mobile Workflow"); `/new-ffi-method <name>` scaffolds it.
This file covers only the mobile-specific parts.

---

## FFI calling rules (both platforms)

- **Sync exports** (the default) block on app-core's global Tokio runtime. Call them off the
  main thread: iOS `Task.detached { try core.method() }.value`, Android
  `withContext(Dispatchers.IO) { core.method() }`.
- **Long-wait exports are native async** — `nextEvents()` and
  `waitForConnectionStateChange()`. `await` them directly (Swift `try await`, Kotlin suspend).
  **Never** wrap them in `Task.detached` or `Dispatchers.IO`, and never add a sync export that
  can block indefinitely (root `CLAUDE.md`, pattern 4).
- All FFI types must be UniFFI-compatible: `String`, `i64`, `bool`, `Vec<T>`, `Option<T>`,
  custom Record/Enum. Never hold an async lock across the FFI boundary.
- Views never call `AppCore` directly — always through the service protocol/interface
  (`AppCoreProtocol` on iOS, `ActnetService` on Android), which has a mock for previews/tests.

---

## iOS

Swift/SwiftUI project under `mobile/ios/` (app target `Actnet/`, plus
`NotificationServiceExtension/`, `ShareExtension/`, and `Shared/` App Group code).

```bash
make ios      # incremental: UniFFI bindings + XCFramework + Xcode project, then build
make xcode    # prepare bindings + xcframework + xcodeproj for an already-open Xcode
make bindings # regenerate UniFFI Swift/Kotlin glue only
```

`make ios` does the minimum work based on file dependencies; it's safe to run on every change.

| File | Purpose |
|---|---|
| `Sources/Services/ActnetService.swift` | `AppCoreProtocol` definition + live implementation |
| `Sources/Services/AppCoreProtocol+Defaults.swift` | Default implementations for protocol methods |
| `Sources/Services/MockActnetService.swift` | Stub for previews/tests |
| `Sources/App/AppState.swift` | Top-level observable state; one core per account in `cores` |
| `Sources/Utils/AvalancheColors.swift` | Semantic color tokens (use these, not system colors) |
| `project.yml` | XcodeGen project definition (source of truth for the Xcode project) |

---

## Android

Kotlin/Jetpack Compose project under `mobile/android/`. Implementation notes:
`docs/60-android-implementation.md`.

```bash
make android            # bindings + native libs, then Gradle builds the debug APK
make android-bindings   # prep only: Kotlin UniFFI glue + per-ABI libapp_core.so into
                        # app/src/main/jniLibs/ (no Gradle). The Android analog of `make xcode`.
```

The Rust cross-compile only reruns when Rust sources change. Gradle needs JDK 17+ (the
Makefile falls back to Android Studio's bundled JBR if `JAVA_HOME` is unset). The core is
consumed as UniFFI-generated Kotlin in `mobile/android/Generated/` plus `libapp_core.so`
loaded via JNA — not an AAR. Both are gitignored build artifacts.

- Per-account event and connection loops run in `viewModelScope` and are cancelled when the
  ViewModel clears.
- SQLCipher DB keys come from the Android Keystore (`KeystoreKeyManager`).
- Min SDK 26 (Android 8.0).

---

## Visual reference

`docs/screenshots/` is the place for iOS simulator screenshots by screen name (it currently
holds only a README). When porting a screen, use a matching screenshot if one exists;
otherwise derive the layout from the iOS source.

## Adding or changing a screen (checklist)

- [ ] iOS SwiftUI view created/updated
- [ ] Android Compose screen created/updated (and Desktop — see `desktop/CLAUDE.md`)
- [ ] `AppState` (iOS) and `AppViewModel` (Android) updated consistently
- [ ] New model fields added to both `.swift` and `.kt` types
- [ ] Styled with design tokens on both platforms (root `CLAUDE.md`, "Design-check")
- [ ] `docs/62-feature-parity.md` updated
