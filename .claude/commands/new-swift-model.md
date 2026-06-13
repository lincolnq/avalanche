Scaffold a new Swift model named `$ArgumentsPascalCase`.

## Step 1 — Create the model file

Create `mobile/ios/Actnet/Sources/Models/$ArgumentsPascalCase.swift`:

```swift
import Foundation

struct $ArgumentsPascalCase: Identifiable, Hashable {
    // Use `let` for immutable fields, `var` for mutable ones.
    let id: String           // unique identifier — DID, UUID, or domain key
    var fieldName: FieldType

    // Computed properties for derived values:
    // var displayDate: Date { Date(timeIntervalSince1970: Double(timestampMs) / 1000.0) }
}
```

Conventions:
- Conform to `Identifiable` for use in `List` / `ForEach` — `id` must be a stable, unique value
- Conform to `Hashable` for use in `NavigationPath`, `Set`, and `Dictionary` keys
- Timestamps from FFI are `Int64` epoch milliseconds — expose them as `Date` via a computed property
- Byte arrays from FFI are `Data` — keep them as `Data` in the model
- Use value types (`struct`) not reference types (`class`) unless you specifically need reference semantics
- Keep models pure data — no network calls, no AppState references

If this model wraps an FFI record type (e.g. `SomeFfi` from UniFFI), add a convenience initializer:
```swift
init(_ ffi: SomeFfi) {
    self.id = ffi.id
    self.fieldName = ffi.fieldName
}
```

## Step 2 — Use in AppState or views

If this model is held in `AppState`, add the property there:
```swift
@Published var items: [String: $ArgumentsPascalCase] = [:]  // keyed by id
```

## Step 3 — Verify

Confirm no Swift syntax errors. Run `make xcode` if on macOS to verify the build.

Report: the file path, the `id` type, and whether it wraps an FFI type.
