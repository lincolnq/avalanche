# 63 — Desktop auto-update

> **Status:** Proposed — needs project-owner review. Nothing is built: Desktop has no
> release pipeline, no bundling, and no updater (`61`).
> **Last verified against code:** 2026-10-04

## Summary

Installed copies of Avalanche Desktop should update themselves, so a security fix reaches
everyone instead of only people who re-download. This doc proposes using Tauri's updater
plugin with update manifests published alongside each GitHub Release, signed by an
Avalanche-held update key that is separate from Apple's code signing. It records the
decisions that are hard to change once copies are installed: where the app looks for
updates, which key it trusts, and how a release becomes visible to clients.

## Proposed

### How it works

- **Plugin.** `tauri-plugin-updater` (Tauri 2). The app fetches a small JSON manifest,
  compares its version with the running one, downloads the platform's update package,
  verifies its signature against a public key compiled into the app, and installs it.
- **Manifest.** One `latest.json` per release, in Tauri's static format:
  `{ version, notes, pub_date, platforms: { "darwin-aarch64": { url, signature }, … } }`.
  The release workflow generates it and attaches it to the GitHub Release along with the
  update packages (`.app.tar.gz` on macOS; `.msi`/AppImage when those platforms ship).
- **Version.** The app's version is the git tag (`v0.6.0` → `0.6.0`), stamped at build
  time like iOS and Android (`Makefile` `MARKETING_VERSION`). Server and Desktop share the
  tag, so every release produces a Desktop build even when Desktop didn't change; the
  updater simply offers it.
- **Only newer versions.** The default semver comparison installs strictly newer versions,
  so a stale or replayed manifest can't downgrade anyone.

### Where the app looks (endpoint)

The endpoint is compiled into every installed copy, so it is effectively permanent: old
copies will keep asking that URL for as long as they run.

**Proposal: a URL on our own domain that redirects to GitHub.**

- Primary: `https://theavalanche.net/desktop/update/latest.json`, a Cloudflare redirect
  (the site already deploys there, `web/CLAUDE.md`) to
  `https://github.com/lincolnq/avalanche/releases/latest/download/latest.json`.
- Owning the URL lets us move hosting later (another repo or org, a CDN, a mirror)
  without stranding installed copies. Pointing straight at GitHub would tie every copy to
  this repo's name and owner.
- Tauri accepts a list of endpoints and tries them in order; the direct GitHub URL is a
  reasonable second entry, so a site outage doesn't stall updates.
- Trust does not come from the host: the signature check (below) is what makes an
  update genuine. The endpoint only decides what is *offered*.

### Which updates are trusted (signing key)

- Updates are signed with a dedicated **updater keypair** (Tauri uses minisign). The
  public key is compiled into the app; the private key signs update packages at release
  time. It is separate from the Apple Developer ID certificate: notarization proves Apple
  vetted the binary, the updater key proves *we* released it.
- **Where the private key lives** is the main security decision:
  - **Option A (recommended to start): a GitHub Actions secret in a protected
    `release` environment** that requires your approval before the job can read it.
    Releases stay one-step (tag, approve), and a stolen GitHub session alone can't sign
    without that approval. Weakness: someone with full control of the repo settings could
    change the protection.
  - **Option B: sign on your machine.** CI builds unsigned packages; you run one command
    that downloads them, signs them with a key in 1Password, and uploads the signatures
    and manifest. The key never touches GitHub. Weakness: a manual step on every
    release, and releases stall when you're away.
- **Backup.** Keep the private key (and its password) in 1Password. Losing it means no
  installed copy can ever be updated again; users would have to re-download.
- **Rotation.** To change keys, ship one release signed with the *old* key whose binary
  contains the *new* public key. After that, sign with the new key. A key that leaks
  must be rotated this way immediately; there is no other revocation.

### Making a release visible (rollout)

- The release workflow already creates **draft** GitHub Releases. A draft is invisible to
  `releases/latest`, so building never ships anything. **Publishing the draft is the
  "ship" button** for Desktop updates and for `avalanche-update` alike.
- Prerelease tags (`v0.7.0-rc.1`) are skipped by `releases/latest`, which leaves room for
  a beta channel later (a second manifest URL for opted-in testers). Not in the first
  version.
- No staged percentage rollout at first; the draft gate plus testers on a pre-release is
  enough at this size.

### In the app

Modeled on Signal Desktop: updates are automatic, with no setting to turn them off.

- **Check** shortly after launch and then every 4 hours while running. Never in dev
  builds.
- **Download in the background** when an update exists. Nothing interrupts the user.
- **Prompt to restart.** Once downloaded, show a small, persistent "Update ready —
  Restart" control (at the bottom of the sidebar, styled with the existing tokens). The
  app never restarts on its own, since that could interrupt typing or a device link.
  Ignored updates install on the next quit-and-launch.
- **Settings → About** shows the current version and a "Check for updates" button with
  the result ("Up to date" / "Downloading…" / "Restart to update" / the error).
- **Failures are quiet.** A failed check or a bad signature is logged and retried at the
  next interval; it never blocks using the app. A signature failure is never "retried
  around": the package is discarded.

### Privacy

Each check reveals the user's IP address and current version to Cloudflare (the redirect)
and GitHub (the manifest and package). This is the same exposure as visiting the website
or downloading the app, and it reveals nothing about the user's account, servers, or
contacts. It should be listed in `09` as a third-party metadata exposure.

### Out of scope here

- Signing and notarizing the macOS app (needed before any of this ships, and covered by
  the Desktop release plan) and Windows code signing.
- Forced or minimum-version updates. If a server ever needs to refuse very old clients,
  that is a protocol-level change, not an updater feature.
- Delta updates; the full package is a few tens of MB.

### Build order

1. Generate the updater keypair; store it in 1Password; add the public key to
   `tauri.conf.json` and the private key per the chosen option (A or B).
2. Enable bundling and version stamping; add the macOS build (signed, notarized) to
   `release.yml`, emitting the update package, its signature, and `latest.json`.
3. Add the Cloudflare redirect for the endpoint.
4. Add the plugin and the in-app UI (sidebar "Restart to update", Settings → About).
5. Test end to end with two pre-release tags: install the first, publish the second,
   watch it update; then tamper with a signature and confirm it's rejected.

## Rationale and rejected alternatives

- **Tauri updater over Sparkle / OS app stores.** The plugin is cross-platform, already
  in our stack, and verifies its own signatures. Sparkle is macOS-only; the Mac App Store
  forces the sandbox and review delays, and its updates are Apple's to schedule.
- **GitHub Releases as storage.** The release workflow already publishes there, and
  drafts give us a free ship gate. Running our own update server would add a service to
  secure for no benefit while signatures carry the trust.
- **Rejected: a raw GitHub URL as the only endpoint.** It can never change once copies
  are installed; see "Where the app looks".
- **Rejected: a user-facing "disable updates" setting.** For a security app, a stale copy
  is the bigger risk. People who must control updates can block the endpoint at the
  network level.
- **Rejected: silent auto-restart.** It risks losing a half-written message or breaking
  a device link in progress; a visible "Restart to update" costs one click.
