# Invite Tokens

Invite tokens encode the information a new user needs to join a server. They are embedded in shareable URLs (`https://go.theavalanche.net/invite/<token>`) that can be shared as links, QR codes, or pasted into the app.

## Token format

A token is `base64url(json)`. The only field the client needs to parse out is `server_url`:

```json
{
  "server_url": "https://myorg.example.com",
  ...
}
```

Tokens may contain arbitrary additional fields. They are passed through to the server, which interprets them (typically via a Project). 

### URL format

```
https://go.theavalanche.net/invite/<base64url_token>
```

## Flow

1. User receives a URL (link, QR code, or paste).
2. App extracts `<token>` from the URL path.
3. App decodes the base64url JSON to extract `server_url`.
4. App calls `GET <server_url>/v1/invites/<token>`. The server decodes the token, performs any validation it wants (signature checks, expiry, usage limits — all server/Project concerns), and returns:
   ```json
   {
     "server_name": "My Org",
     "server_step_url": "https://myorg.example.com/p/onboarding?token=...",
     "post_onboarding_redirect": "https://go.theavalanche.net/conversation/did:plc:abc123"
   }
   ```
   All fields except `server_name` are optional.
5. App shows "Join [server_name]?" screen (identity picker if existing accounts, new account flow otherwise).
6. User registers on the server (normal `POST /v1/accounts` flow, with the raw token passed through in an `invite_token` field).
7. If `server_step_url` is present, the app opens it in a webview (the "server step" from doc 33). The Project handles whatever onboarding it needs — collecting a name, assigning teams, showing terms of service, etc.
8. If `post_onboarding_redirect` is present, the app navigates to that deep link. For example, `https://go.theavalanche.net/conversation/<inviter_did>` opens a DM with the person who invited you. (Probably the server step should also have control over this post onboarding redirect, but we will implement that later.)

## Current implementation shortcut

Until the Project framework exists, the server handles one token field directly: if the token contains `inviter_did`, the server's `GET /v1/invites/<token>` response includes `post_onboarding_redirect` set to `https://go.theavalanche.net/conversation/<inviter_did>`. This gives us the "scan invite, register, land in a DM" flow without needing Projects. Once Projects exist, this behavior moves to an invite Project.

## Creating tokens

Tokens are constructed client-side — the app base64url-encodes the JSON payload directly. No server endpoint is needed.

For example, to create an invite link for a dev server:
```bash
echo -n '{"server_url":"http://localhost:3000"}' | base64 | tr '+/' '-_' | tr -d '='
# Paste the result into: https://go.theavalanche.net/invite/<result>
```

A server-side endpoint for generating invite links from within the app is a future Project concern.

### "My QR Code" screen

The app includes a screen (in Settings for now) that displays a QR code encoding the user's personal invite link. The token payload is `{"server_url":"<user's server>","inviter_did":"<user's DID>"}`. Scanning this QR code registers the new user on the same server and opens a DM with the inviter. The QR code is generated client-side — no server call needed.

If the scanning user already has an account on that server, the app skips registration and the onboarding flow entirely and navigates directly to a DM with the inviter.

## Security

In the current implementation, tokens are not signed. Anyone who knows a server URL can construct a valid token. This is fine because registration is open — the token is a convenience for discovery, not an access control mechanism.

Signing, expiry, usage limits, and closed registration are all server/Project concerns. A Project can sign tokens with its own secret, validate them during the `GET /v1/invites/<token>` call, and reject invalid or expired tokens. The substrate doesn't need to know about any of this.

## What the substrate owns vs. what Projects own

**Substrate:**
- Decode the base64url JSON
- Extract `server_url`
- Call the server's validation endpoint, passing the raw token through
- Display the server name and registration UI
- Open the server step webview if provided
- Navigate to the post-onboarding redirect if provided

**Projects (future):**
- Token signing and validation
- Expiry and usage limits
- Onboarding flows (server step webview content)
- Auto-enrollment into groups
- Post-onboarding redirect (e.g., open a DM, navigate to a channel)
- In-app invite creation UI
