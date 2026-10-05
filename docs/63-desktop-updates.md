# 63 — Desktop auto-update

> **Status:** Planned — owner-approved 2026-10-04 (endpoint on our domain; signing key in
> 1Password, used on the maintainer's machine like the Android release key). Being built.
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
- **Platforms.** macOS Apple Silicon, Windows x64, Linux x64. Intel Macs come later
  (a universal build); Windows and Linux mattered more.
- **Who builds what.** Tauri needs a native machine per OS. The release workflow's
  `desktop-build` job builds the Windows NSIS installer and the Linux AppImage + `.deb`
  on tag push, with no update signature, and attaches them to the draft release. The
  macOS app is built, Developer ID signed, and notarized on the maintainer's Mac by
  `make desktop-release`, which also downloads the Windows/Linux installers from the
  draft and signs them with the updater key. Building unsigned packages in CI doesn't
  weaken anything: a package is only an update once that local step signs it.
- **Manifest.** One `latest.json` per release, in Tauri's static format:
  `{ version, notes, pub_date, platforms: { "darwin-aarch64" | "windows-x86_64" |
  "linux-x86_64": { url, signature } } }`. `make desktop-release` writes it, listing every
  platform whose package it signed (a platform whose CI build failed is left out, so the
  others still ship), and uploads it with the signatures.
- **What updates on Linux.** The updater can replace an AppImage. The `.deb` is offered
  for people who prefer a system package and is updated by reinstalling a newer `.deb`.
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
- **Where the private key lives: 1Password, used only on the maintainer's machine**
  (decided), the same way the Android release keystore works
  (`mobile/android/release-sign.sh`). `make desktop-release` reads the key and its
  password from 1Password into environment variables for the length of the build, so it
  never lands on disk and never reaches GitHub. Consequently the Desktop release is built
  and signed locally, like `make android-release` and the iOS `make archive`, and its
  artifacts are uploaded to the tag's GitHub Release; CI builds the server side only.
  (Rejected: a GitHub Actions secret behind a protected environment. One-step releases,
  but the key would sit with GitHub, where anyone with full control of the repo settings
  could reach it.)
- **Backup.** 1Password is the only copy, so it must stay backed up there. Losing it means
  no installed copy can ever be updated again; users would have to re-download.
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

- Windows code signing. Unsigned installers work, but Windows SmartScreen warns
  ("Windows protected your PC") until the app has reputation. Fine for testers; sign
  before a wider launch (Azure Trusted Signing is the cheapest current route).
- Forced or minimum-version updates. If a server ever needs to refuse very old clients,
  that is a protocol-level change, not an updater feature.
- Delta updates; the full package is a few tens of MB.

### Build order

1. Generate the updater keypair straight into 1Password (`desktop/updater-keygen.sh`);
   add the public key to `tauri.conf.json`.
2. Enable bundling and version stamping; `make desktop-release` builds the macOS app
   (Developer ID signed, notarized), signs the update package with the 1Password key, and
   uploads it, its signature, and `latest.json` to the tag's GitHub Release.
3. Add the redirect for the endpoint (`web/static/_redirects`).
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
