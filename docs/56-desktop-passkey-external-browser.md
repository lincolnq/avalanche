# 56 — Desktop passkeys via the external browser

> **Status:** Proposed — spec only; nothing is built. Desktop signup and recovery use the recovery phrase. Needs project-owner review before implementation, and should be re-checked against `50` §Proposed (P1 changes which RP domains and what the PRF unlocks).
> **Last verified against code:** 2026-10-03

## Summary

Give the Tauri desktop app a real passkey flow for signup and recovery on Windows, macOS, and Linux by running the WebAuthn ceremony in the user's default browser on a page hosted at the passkey RP domain, and handing the PRF result back to the app over a `127.0.0.1` loopback channel (the RFC 8252 pattern). Passkey becomes the default desktop signup credential; the phrase stays as a fallback. This removes the sanctioned divergence in `desktop/CLAUDE.md` ("Passkey / recovery divergence").

## Current state

Desktop has no passkey path: `desktop/src-tauri/src/lib.rs` exposes `derive_did_from_passkey` but the only caller is the phrase flow, and the bridge page (`web/static/webauthn-bridge/`) does not exist. The phrase seed passes through the app's JS layer today.

## Design (Planned)

### Why the external browser

Three approaches were considered:

- **In-app WebView `navigator.credentials` (rejected).** webkit2gtk has no WebAuthn; WKWebView needs a browser-only entitlement; only WebView2 works.
- **Native OS ceremony in Rust (deferred).** Three separate integrations (`webauthn.dll`, an `ASAuthorization` shim, `libfido2`); Windows native callers get no app↔domain binding; Linux can't reach software providers like 1Password.
- **External browser + loopback handback (chosen).** One implementation on every OS, every provider and extension the user has, real origin binding enforced by the browser, ordinary portable web passkeys that produce the same PRF and DID as mobile. Cost: a browser bounce, acceptable for rare, high-stakes flows.

### Contract that must match mobile

The browser ceremony must produce PRF bytes identical to iOS/Android for the same passkey:

- RP ID `theavalanche.net` (iOS `PasskeyManager.relyingParty`, Android `PasskeyManager.RELYING_PARTY`).
- PRF salt `actnet-recovery-v1`.
- `user.id` = signup server URL bytes.
- Discoverable credential (`residentKey: required`).

If the spike shows the browser PRF differs from mobile, that is a blocker, not a reason to fork the contract.

### Flow

```
Desktop app (Tauri)                    Default browser               RP domain
1. bind 127.0.0.1:<port> (first)
2. ephemeral X25519 keypair
3. open https://<rp>/webauthn-bridge/?v=1&op=register|authenticate&port=..&pk=..&nonce=..[&user=..&name=..]
                                       4. load page (real RP origin)
                                       5. navigator.credentials.create/get with prf.eval.first = salt
                                       6. PRF (+ userHandle on authenticate)
                                       7. seal payload to pk
8. <- POST http://127.0.0.1:<port>/callback {v, op, nonce, sealed}
9. verify nonce, decrypt in Rust -> PRF (never enters the app WebView)
10. app-core create_account / recover_from_blob
11. return a non-secret AccountSummary to the frontend
```

Sealing: page-side ephemeral X25519 + HKDF-SHA256 + AES-256-GCM using WebCrypto only; Rust side uses RustCrypto crates. libsodium.js `crypto_box_seal` is a fallback for browsers without WebCrypto X25519. On `create`, if the provider returns no PRF, do a follow-up `get` on the new credential (create-then-get). Errors POST `{cancelled | prf_unsupported | no_credential | unknown}` so the app can fall back to the phrase.

### Components

- **Web page** `web/static/webauthn-bridge/{index.html, ceremony.js}` — parse and validate params, run the ceremony, seal, POST to loopback, show "return to Avalanche".
- **Tauri backend** `desktop/src-tauri/src/passkey.rs` — single-shot loopback listener on `127.0.0.1:0` (~120 s timeout), seal-open, browser launch via `tauri-plugin-opener` scoped to the bridge URL only. Commands: `passkey_register`, `passkey_authenticate` (derives the DID, resolves the home server, stashes the PRF in a mutex-guarded pending slot), `passkey_recover_finish`.
- **app-core** — no changes; reuses `create_account`, `recover_from_blob`, `derive_did_from_passkey`, `resolve_homeserver_from_plc`.
- **Frontend** — service methods `passkeyRegister`, `passkeyAuthenticate`, `passkeyRecoverFinish` (+ mock); passkey as the primary signup and recovery button, phrase as secondary; all paths finish through the shared `enterApp()`; back navigation through `OnboardingFlow`'s back-stack; co-located CSS with theme tokens.

### Security model

- **PRF in transit** is sealed to the app's ephemeral key; a process sniffing the loopback sees ciphertext.
- **Port hijack**: the app binds before launching the browser.
- **No custom URL scheme**: any app can register one, and the payload would leak into browser history.
- **The `Origin` check only filters browsers.** A local process can send any `Origin` header, and the launch URL (with `pk` and `nonce`) is visible to other local processes via the browser's command line. During **signup**, a malicious local process could race the browser and POST its own sealed "PRF", making the new identity's root secret attacker-known. Local malware is generally out of scope, but the window should be minimized (accept exactly one POST, short timeout) and the app should confirm with the user that the browser step completed before using the result.
- **The RP domain is the trust base.** Any `*.theavalanche.net` origin can run ceremonies for this RP with the fixed salt, and the demo homeserver serves third-party Project code under that domain today (`50` Known gaps). The bridge page must be served from an origin with no third-party content, and the broader fix is in `50` Planned (dedicated RP domain or no third-party content under the RP).
- Accepted residual risks: a malicious browser extension can read the PRF in-page (same as any web login); a local app could launch the flow to phish an approval (gated by the provider's user verification).

### Phase 0 — feasibility spike (gates everything)

1. PRF from the bridge page is stable for the same passkey and salt.
2. PRF is bit-identical to iOS/Android for the same passkey (create on desktop with 1Password, recover on mobile; compare DIDs).
3. Create-then-get yields a PRF with 1Password, iCloud Keychain, and a hardware key.
4. Choose WebCrypto vs libsodium sealing from target-browser coverage.

If (2) fails, stop: the shared-RP recovery model needs rethinking.

### Test plan

- Rust: loopback accepts one valid POST and decrypts; rejects wrong nonce; times out cleanly; sealed-box test vectors.
- Mock frontend: signup and recovery complete with no browser and converge on `enterApp()`; cancel falls back to phrase.
- Manual matrix: Windows (1Password, hardware key), macOS (iCloud, 1Password, hardware key), Linux (Chrome+1Password, Firefox+hardware key); cross-device desktop↔mobile recovery; PLC failure fallback to a typed server URL; cancel; port in use; second POST rejected.

## Open questions

1. PRF bit-equivalence across providers and platforms (the spike).
2. Sealing choice and the site's CSP.
3. Provider support for PRF at create vs. create-then-get.
4. Returning focus to the app (tabs usually can't self-close).
5. GNOME Web / webkit2gtk default browsers fail; guide to the phrase fallback.
6. Self-hosted deployments depend on the RP domain hosting the bridge page; under `50` P1 there may be two RP domains to host it on.

## Speculative

- **Native in-app ceremony** where the bounce chafes (Windows `webauthn.dll`, macOS `ASAuthorization`, `libfido2`), coexisting with the browser path.
- **Keep the phrase seed out of the WebView too**, matching the PRF-stays-in-Rust posture.
