# 51 — Invite tokens

> **Status:** Built — personal invite links (unsigned, carrying a server and optionally an inviter DID) and Project-signed gatekeeper tokens for closed registration are both built. The onboarding "server step" webview and token-driven group auto-enrollment are not.
> **Last verified against code:** 2026-10-03

## Summary

An invite token tells a new (or existing) user which server to join and, optionally, who invited them. Tokens are `base64url(JSON)` with single-character keys to keep QR codes small, embedded in `https://go.theavalanche.net/i/<token>` links. Two shapes exist: an unsigned **personal invite** (discovery only) and a Project-signed **gatekeeper token** (admission to a closed-registration server, `24`). A third shape, the operator's **bootstrap token**, carries the server's shared registration secret.

## Known gaps

- **No server step.** `GET /v1/invites/{token}` returns `server_name`, an optional `post_onboarding_redirect`, and an optional `privacy_policy_url`; it never returns a `server_step_url`, and the apps have no server-step webview (`50` onboarding screens).
- **No group auto-enrollment from tokens.** Nothing reads a `group_invitations` field.
- **Tokens travel in the URL path.** When Universal Links / App Links don't fire, the token lands in the `go.theavalanche.net` web server's access logs. Today's personal tokens carry only a server URL and a DID, but any future token carrying a secret (group master keys, link passwords) must not use the path. Signal puts such secrets in the URL fragment, which browsers never send to the server.
- **Bootstrap tokens contain the server's master registration secret** and are handed to Project operators as "setup codes"; anyone holding one can edit the Project slug and register into the superuser Project. See `24` and `09`.
- **`GET /v1/invites/{token}` only understands personal tokens.** It decodes `{s, d}` and checks `s` matches the server; it does not verify gatekeeper signatures (that happens at registration).

## Current design

### Link format

```
https://go.theavalanche.net/i/<base64url token>
```

The legacy `/invite/<token>` path is still accepted (`mobile/ios/.../AppState.swift`). The host is matched in-app, so links work even when Universal Links don't fire inside the app (`23`).

### Personal invite token

**Built.** `{"s": "<server_url>", "d": "<inviter_did>"}`; `d` is optional. Generated client-side with no server call by the identity detail screen's contact QR code and link (`IdentityDetailView.swift`). The same link doubles as a **contact link**: pasting or scanning it into the compose recipient field adds the `d` DID as a recipient (`ComposeMessageView.swift` `recipientDid(fromContactLink:)`, which also accepts `/conversation/<did>`).

Flow:

1. The app extracts the token from the URL and decodes `s`.
2. `GET <s>/v1/invites/<token>` (`server/src/routes/invites.rs`). The server checks `s` matches its own URL and returns `{server_name, post_onboarding_redirect?, privacy_policy_url?}`. If `d` is present, `post_onboarding_redirect` is `https://<invite_domain>/conversation/<d>`.
3. The app shows "Join <server_name>?" with the identity picker if identities exist (`50`).
4. New identity: register with `POST /v1/accounts`, passing the raw token as `invite_token`.
5. If `post_onboarding_redirect` is present, the app opens it, which lands the user in a DM with the inviter.

If the scanning user already has an account on that server, the app skips registration and goes straight to the DM.

On a **closed-registration** server (the default, `24`), a personal token does not admit a new account; only a gatekeeper or bootstrap token does.

### Gatekeeper token

**Built** (`server/src/invite_token.rs`). An envelope `{"s", "i": <issuer project slug>, "c": base64url(claims), "g": base64url(Ed25519 sig over c)}`. Claims: `{"s", "i", "e": exp, "j": jti, "u": purpose ("invite"), "r": routing?}`. The server picks the pinned key by issuer, requires the issuer to hold `registration.gatekeeper`, verifies the signature, checks `server_url`, expiry, and purpose, and redeems `jti` once. The opaque `routing` payload is carried through to the account-joined event for post-join routing. Details in `24`.

### Bootstrap token

**Built.** `{"s", "k": <REGISTRATION_SHARED_SECRET>, "p": <project slug>?}`. Admits registration while the shared secret is configured and no gatekeeper is installed, and links the new account into the named Project. Used to bootstrap adminbot (superuser) and hand Projects their bot accounts. See `24` for why this shape must change.

## Planned

- Replace bootstrap-token setup codes with server-minted, per-Project, single-use bot-enrollment tokens (`24`, `09`).
- Server step: return `server_step_url` from invite validation and open it as an onboarding webview between registration and landing.
- Group auto-enrollment from gatekeeper tokens (`group_invitations: [{master_key, link_password}]`, applied after registration via `join_with_link` per `03` §3.10). Must carry secrets in a URL fragment, not the path.
- In-app invite creation for admins, via a Project.

## Speculative

- **Deferred deep links through install** (Proposed in `23`; listed here for context). A `/project/<t>` or invite link that survives an App Store install and lands the new user in the Project or group after onboarding. This is the acquisition path the product premise depends on (`00`); see `20`.

## Rationale and rejected alternatives

- **Unsigned personal tokens (decided).** A personal token is a discovery convenience, not access control; anyone who knows a server URL could construct one. Admission control lives in gatekeeper tokens.
- **Signing in the substrate vs. in Projects (decided: Projects sign).** The server pins each gatekeeper Project's public key and verifies locally; it never calls the Project (`24`).
- **Single-character JSON keys (decided).** Keeps QR codes small enough to scan reliably.
