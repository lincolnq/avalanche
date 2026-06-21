# Desktop Manual QA Checklist

Check these off manually after each day's loop completes. Run `cargo tauri dev` in
`desktop/` unless noted otherwise. Use **mock mode** (the default) for everything
unless the item says "devServer mode."

This file is for manual verification — the loop agent does not modify it.

---

## Day 1 — Scaffold · Models · Services · AppContext · Navigation

### App opens
- [ ] `cd desktop && cargo tauri dev` runs without compile errors
- [ ] Tauri window appears (not blank — at minimum shows a heading or text)
- [ ] No JS console errors on load (`F12` → Console)
- [ ] `Object.freeze(Object.prototype)` runs without throwing (no "Cannot assign to read only" errors in console)

### Mock mode — conversation list
- [ ] In mock mode, switching to main layout shows at least 3 seeded conversations
- [ ] Each row shows: name, last message preview
- [ ] Sidebar has **Chats** and **Network** links (no Calls link — it was removed)
- [ ] Clicking Chats shows the conversation list
- [ ] Clicking Network shows the network stub view
- [ ] Active route is visually highlighted in the sidebar

### Onboarding gate
- [ ] On first launch (no persisted accounts), `SplashView` is shown, not the main layout
- [ ] "Enter Invite Link" button is visible on `SplashView`

---

## Day 2 — Onboarding Flow

### Invite link entry
- [ ] "Enter Invite Link" opens `InviteLinkEntryView`
- [ ] Pasting `https://server.example.com/invite/ABC123` parses without error
- [ ] Pasting a malformed link shows an inline error message
- [ ] Back navigation returns to `SplashView`

### QR scanner
- [ ] "Scan QR Code" button opens `QRScannerView`
- [ ] File upload button is visible (desktop primary path)
- [ ] Uploading an image file attempts to decode a QR code

### Identity picker → account creation (mock mode)
- [ ] After parsing a valid link, `IdentityPickerView` appears with a "New identity" option
- [ ] Selecting "New identity" opens `NewAccountView`
- [ ] Display name field is focused / visible
- [ ] Submitting an empty display name is blocked or shows an error
- [ ] Submitting "Alice" (any name) shows `JoiningServerView` spinner
- [ ] After the mock join, the main layout appears with sidebar
- [ ] The account name "Alice" appears somewhere (sidebar, settings, etc.)
- [ ] Re-launching the app (stop and restart `cargo tauri dev`) restores the session — main layout shown, not onboarding

---

## Day 3 — Chats Tab

### Conversation list
- [ ] At least 3 mock conversations listed
- [ ] Rows show: avatar (initials), name, last-message preview, relative timestamp
- [ ] Conversations with unread messages show a count badge
- [ ] List is sorted newest-first

### Conversation view
- [ ] Clicking a conversation opens `ConversationView`
- [ ] Message history is displayed
- [ ] Own messages appear on the **right**, received on the **left**
- [ ] Each message shows a timestamp
- [ ] Edited messages show an "edited" marker
- [ ] Deleted messages show a tombstone (greyed out placeholder), not empty space

### Delivery indicators
- [ ] After sending, the new message briefly shows a "sending" indicator (clock/grey)
- [ ] Indicator updates to "sent" (single check) after mock acknowledgement

### Compose
- [ ] Text field is present at the bottom of the conversation
- [ ] Typing a message and clicking Send appends it optimistically to the list
- [ ] Compose field clears after send
- [ ] Sending an empty message is blocked

### Unread / read
- [ ] Opening a conversation clears the unread badge for that conversation
- [ ] Total unread count in the sidebar (Chats icon) updates

### Avatar shapes
- [ ] Human contacts render with a **circle** avatar frame
- [ ] Bot accounts render with a **hexagon** avatar frame (if any bots in mock data)

### RecoveryKeyBanner
- [ ] A banner prompting recovery-phrase setup is visible somewhere in the chats tab when no recovery phrase is configured

---

## Day 4 — Network Tab · Rust Bridge

### Network tab (mock mode)
- [ ] NetworkView lists at least one server from the mock account
- [ ] Each server shows its name and URL
- [ ] Mock projects are listed under the server

### ProjectWebView
- [ ] Clicking a project opens a modal window (not an in-page navigation)
- [ ] The modal loads the project URL
- [ ] The modal has a close button / can be dismissed
- [ ] Navigating to `go.theavalanche.net/...` inside the modal fires a deeplink (check Rust logs)

### devServer mode — real server
> These require `make dev-all` running in another terminal.
- [ ] Switching to "Dev Server" mode in DevSettings resets to onboarding
- [ ] Pasting a dev invite link (from `dev.py`) parses correctly
- [ ] Creating an account completes without error (check Rust logs)
- [ ] Sending a DM to testbot receives a reply
- [ ] Messages persist across app restart (reload `cargo tauri dev` — history shown)
- [ ] Connection state banner shows when the server is stopped (`make dev-all` killed)
- [ ] Banner clears when the server restarts

---

## Day 5 — Settings · Polish

### DevSettingsView
- [ ] Accessible (button in sidebar footer, gear icon, or similar)
- [ ] Shows current mode (mock / devServer)
- [ ] Mode switcher changes mode and resets to onboarding
- [ ] Shows account count and server URL

### AccountsView
- [ ] Lists all signed-in accounts
- [ ] "Leave server" option is present
- [ ] Confirming "Leave server" removes the account and returns to onboarding (mock mode)

### Deep links
- [ ] Clicking an `actnet://invite/TOKEN` link from outside the app opens it (requires registration in tauri.conf.json)
- [ ] The invite token is handed to the onboarding flow

### Notifications (devServer mode)
- [ ] Receiving a DM while the app is in the background fires a native OS notification
- [ ] Notification does not fire when the conversation is already open

### Windows build
- [ ] `cargo tauri build` completes without errors in `desktop/`
- [ ] Generated `.msi` or `.exe` installer is present in `desktop/src-tauri/target/release/bundle/`
- [ ] Installer runs and app launches

---

## Cross-cutting

These apply to every day's output:

- [ ] No `console.error` output during normal happy-path usage
- [ ] No `any` TypeScript suppressions (`grep -r "as any" desktop/src` returns nothing)
- [ ] `npm run build` (Vite production build) completes without TypeScript errors
- [ ] CSP is strict — open DevTools → Network, confirm no blocked resource warnings from external hosts
