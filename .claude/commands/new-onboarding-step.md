Add a new onboarding step named `$ArgumentsPascalCase` to the iOS app's onboarding flow.

## Step 1 — Create the view file

Create `mobile/ios/Actnet/Sources/Views/Onboarding/$ArgumentsPascalCase.swift` following the pattern of existing onboarding views (e.g. `InviteLinkEntryView.swift`, `RecoveryExplainerView.swift`):

```swift
import SwiftUI

struct $ArgumentsPascalCase: View {
    @EnvironmentObject var appState: AppState

    // Callback to advance to the next step or dismiss
    var onComplete: () -> Void = {}

    var body: some View {
        VStack(spacing: 24) {
            // Content
            Text("$ARGUMENTS")
                .font(.title2)
                .bold()

            Spacer()

            Button("Continue") {
                onComplete()
            }
            .buttonStyle(.borderedProminent)
        }
        .padding()
        .navigationTitle("$ARGUMENTS")
        .navigationBarTitleDisplayMode(.inline)
    }
}

#Preview {
    NavigationStack {
        $ArgumentsPascalCase()
            .environmentObject(AppState.preview)
    }
}
```

## Step 2 — Add navigation in SplashView

Read `mobile/ios/Actnet/Sources/Views/Onboarding/SplashView.swift` to understand the existing navigation pattern.

Add a `@State` flag:
```swift
@State private var show$ArgumentsPascalCase = false
```

Add a trigger (button or automatic):
```swift
Button("Go to $ARGUMENTS") {
    show$ArgumentsPascalCase = true
}
```

Add a `.navigationDestination` block after the existing ones:
```swift
.navigationDestination(isPresented: $show$ArgumentsPascalCase) {
    $ArgumentsPascalCase(onComplete: {
        show$ArgumentsPascalCase = false
        // advance to next step or complete onboarding
    })
}
```

If the step should appear automatically based on app state, use `.onChange`:
```swift
.onChange(of: appState.someCondition) { _, newValue in
    if newValue { show$ArgumentsPascalCase = true }
}
```

## Step 3 — Handle completion

When the step completes, decide the outcome:
- **Advance to next step**: set the next step's `@State` flag to `true`
- **Complete onboarding**: set `appState.isOnboarding = false` (or equivalent)
- **Go back**: leave the `NavigationStack` to pop automatically

## Step 4 — Verify

Run `make xcode` if on macOS. If on Windows, confirm no Swift syntax errors and note that build verification requires a Mac.

Report: the file path, where in `SplashView` the trigger was added, and the completion action.
