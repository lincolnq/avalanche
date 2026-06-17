Scaffold a new SwiftUI view named `$ArgumentsPascalCase` in the iOS app.

## Step 1 — Determine the view's location

Based on the purpose of the view, place it in the correct subdirectory under `mobile/ios/Actnet/Sources/Views/`:

| Subdirectory | Use for |
|---|---|
| `Chats/` | Conversation list, message composition, group detail |
| `Common/` | Reusable components used across multiple screens |
| `Network/` | Server browser, Project webviews |
| `Onboarding/` | Account creation, recovery, invite flow |
| `Settings/` | Account settings, server details, identity |

For a new onboarding screen, use `/new-onboarding-step` instead.

## Step 2 — Create the view file

Create `mobile/ios/Actnet/Sources/Views/<Subdirectory>/$ArgumentsPascalCase.swift`:

```swift
import SwiftUI

struct $ArgumentsPascalCase: View {
    @EnvironmentObject var appState: AppState

    // Add @State, @Binding, or let properties as needed
    // @State private var isLoading = false
    // @State private var errorMessage: String? = nil

    var body: some View {
        // View content
        VStack {
            Text("$ArgumentsPascalCase")
        }
        .navigationTitle("$ARGUMENTS")
        .task {
            // Use .task for async work triggered on appear.
            // Call FFI methods via:
            //   await appState.someMethod()
            // AppState dispatches these via Task.detached internally.
        }
    }
}

#Preview {
    $ArgumentsPascalCase()
        .environmentObject(AppState.preview)
}
```

Key patterns:
- Access global state via `@EnvironmentObject var appState: AppState` — never call FFI directly from views
- Use `.task { }` for async work on appear; use `Button { Task { ... } }` for user-triggered async actions
- For error handling: `@State private var errorMessage: String?` + `.alert(...)` modifier
- For loading state: `@State private var isLoading = false` + `.disabled(isLoading)`

## Step 3 — Register in the parent view

Add navigation to the new view from its parent. Common patterns:

**NavigationLink (list row):**
```swift
NavigationLink(destination: $ArgumentsPascalCase()) {
    // row content
}
```

**Sheet:**
```swift
.sheet(isPresented: $showView) {
    $ArgumentsPascalCase()
        .environmentObject(appState)
}
```

**NavigationStack destination:**
```swift
.navigationDestination(for: SomeType.self) { _ in
    $ArgumentsPascalCase()
}
```

## Step 4 — Verify

Run `make xcode` to confirm the project builds (requires macOS). If on Windows, confirm the Swift file has no obvious syntax errors and note that build verification requires a Mac.

Report: the file path, the parent view it's navigated from, and any AppState properties it reads.
