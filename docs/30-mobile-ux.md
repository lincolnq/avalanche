# Mobile App UX

## First launch

On first launch with no account, the app shows a splash screen with two paths:

- **Scan invite QR code** — opens the camera to scan an invite code
- **Enter invite link** — paste or type a link manually

There is no "create account" flow independent of a server invitation. You always join a server.

## Invite links

Invite links are web URLs on the homeserver's domain (e.g., `https://myorg.example.com/invite/<token>`). QR codes encode the same URL. The link opens in the browser, where the homeserver serves a landing page that:

- Explains what's happening ("You've been invited to join [Org Name]")
- Links to the App Store / Play Store if the app isn't installed
- Has an "Open in Avalanche" button that links to `https://go.theavalanche.net/invite/<server>/<token>`, which opens the app via Universal Links (iOS) / App Links (Android)

## Registration flow

### New user (no existing account)

When the app receives an invite link (either via deep link or QR scan):

1. The app contacts the server and validates the invite token.
2. **Display name screen.** "What's your name?" with a text field and optional photo upload. Display name is required; photo is not. This is the only screen the user must interact with.
3. The app generates keys (identity key, prekeys) and registers with the server in the background. The user never sees this.
4. The server creates the account, generates a DID, and auto-enrolls the user into whatever groups/Projects the invite token specified.
5. **Push notification permission prompt.** iOS requires explicit permission; ask here with context ("Get notified when your team sends a message").
6. The user lands in the **Chats tab** with their groups already populated.

If the invite token specifies an onboarding Project (e.g., a conference registration flow), that Project's onboarding UI runs between steps 4 and 5. The Project can collect additional information (organization, role, dietary restrictions, whatever it needs). The substrate display name is already set; the Project collects Project-specific data.

Total interaction for the minimal case: scan, type name, tap continue, approve push notifications. A few seconds of background work. That's it.

### Existing user (already has one or more accounts)

When the app receives an invite and the user already has accounts:

1. The app shows a choice of identities:
   - **Join as [Alice]**
   - **Create a fresh identity**
   - **More options...**
   The Alice prefill is your most recently used identity. More options presents a list of all your signed-in accounts alongside which servers they are bound to.
2. If joining as an existing identity: the app registers that DID with the new server (signs a challenge to prove ownership, uploads fresh prekeys). One confirmation tap.
3. If creating a new account: full new-user flow — new DID, new name, new identity.
4. Either way, auto-enrollment and onboarding proceed as above.

Creating a separate account is the right choice when you want to keep identities apart — e.g., organizing pseudonymously with one group while using your real name with another. Most users will just tap their existing name.

## Account recovery (passkey)

During initial signup, after entering a display name, the app prompts the user to create a passkey. This is a single biometric prompt (Face ID / fingerprint) — the passkey is stored in the user's password manager or iCloud Keychain and syncs across their devices automatically. The passkey protects an encrypted recovery blob (containing the user's DID rotation key and identity keypair) stored on each homeserver the user is registered on. See `docs/33-identity-auth-recovery.md` for the full design.

## Display name

Display name is attached to a DID, required at account creation. It is what other users see in chats. The name is client-owned — stored locally and pushed to every server the DID is registered on. Changing your name updates it everywhere. This is the same model Signal uses for profile names.

If you want different names in different contexts, create separate accounts (separate DIDs). There are no per-server name overrides — one DID, one name.

## Multi-account

The app supports multiple accounts (multiple DIDs). Each has its own display name, keys, and set of servers. All accounts' chats and servers appear together in the Chats and Network tabs — you don't switch accounts to see different content. Each chat and server has a subtle indicator showing which identity it's associated with.

When you send a message, you send as whichever identity is a member of that group. When starting a new DM with someone reachable from multiple identities, the app asks which identity to use, defaulting to the one that shares a server with the recipient.

### Multiple identities in the same conversation

It is possible to join the same group or server with multiple identities. The app doesn't prevent this, but warns you: "You're already on [Server] as [Alice]. Join as [Bob] too?" The server sees two unrelated accounts and can't tell they're the same person.

When multiple of your identities are present in a conversation, the app shows a small identity indicator with a way to switch. It defaults to whichever identity you last sent from in that conversation. Messages from your active identity render as "you" (right side); messages from your other identities render like any other participant (left side, with name and avatar). Seeing your own name on the left is a strong cue that you may be on the wrong identity.

## Navigation

Three tabs:

- **Calls.** Voice and video calls.
- **Chats.** Unified inbox across all servers, sorted by recency. Every DM and group you belong to appears here. This is the default tab and primary surface.
- **Network.** Hierarchical list of servers you're on. Each server expands to show its Projects. Tapping a Project opens it full-screen with its own navigation.

Projects open as full-screen views. Group chats managed by a Project appear in the Chats tab like any other chat; the Project view is for non-chat surfaces (maps, dashboards, sign-up flows). Projects and chats are deep-linkable in both directions.
