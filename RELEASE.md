# Releasing

All first-party components share **one version**: a single git tag drives both
the server-side release artifacts and the mobile app version.

## 1. Tag the release

Use a `v`-prefixed semver tag, e.g. `v0.2.0`.

From a clean `main` at the commit you want to ship:

```bash
git tag v0.2.0
git push                 # push the branch first, so the tagged commit
                         # (and the release workflow it contains) is on the remote
git push --tags          # push the tag, which triggers the release
```

## 2. Server-side artifacts (automatic, via GitHub Actions)

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds every
first-party server binary for both Linux arches (`x86_64` and `aarch64`) and attaches
them to a **draft** GitHub Release. The same run builds the Windows and Linux
Desktop installers (section 3):

| Asset (per arch)                  | Contents                                  |
| --------------------------------- | ----------------------------------------- |
| `av-server-<target>.tar.gz`       | `avalanche-server` binary                 |
| `av-relay-<target>.tar.gz`        | `relay` binary                            |
| `av-adminbot-<target>.tar.gz`     | self-contained Node bot (node_modules + built packages) |
| `av-testbot-<target>.tar.gz`      | self-contained Node bot                   |

Then:

1. Watch the run: `gh run watch` (or the Actions tab on GitHub).
2. When it finishes, run the Desktop release (section 3) before publishing.
3. Open the **draft release** on GitHub, add release notes.
4. **Publish** the draft as a full release (not a pre-release):
```bash
gh release edit v0.2.0 --draft=false
```

Publishing is what ships the release. Installed Desktop apps check
`releases/latest`, which skips drafts **and pre-releases**, so a release
published with `--prerelease` never reaches them as an update.


## 3. Desktop app (CI + your Mac)

CI builds the Windows installer and the Linux AppImage + `.deb` and attaches
them to the draft, without update signatures. The macOS build and all update
signing happen on your Mac, because the updater key lives only in 1Password
(docs/63). After the tag's release workflow has finished:

```bash
git checkout v0.2.0
make desktop-release
```

This builds the macOS app, signs it with the Developer ID, notarizes and
staples it, then downloads the Windows/Linux installers from the draft, signs
all three with the updater key, writes `latest.json`, and uploads everything to
the draft. Expect 1Password prompts and an Apple notarization wait. If a
platform's CI build failed, it's left out of `latest.json` and the others still
ship.

Run it only after CI finishes, or the Windows/Linux installers won't be on the
draft yet. Test the pipeline without uploading: `desktop/release.sh --local-test`.

Once the release is published, bump `desktopVersion` in `web/hugo.toml` so the
website's Desktop download links point at it (section 6).

## 4. iOS app (manual, from your Mac)

The iOS app version is derived from git tags (`project.yml` has the logic).

- **Marketing version** (`CFBundleShortVersionString`) = the latest tag with the
  leading `v` stripped (so `v0.2.0` → `0.2.0`).
- **Build number** (`CFBundleVersion`) = total commit count (`git rev-list
  --count HEAD`), which always increases — App Store Connect requires the build
  number to be higher than any previous upload.

Build a signed, TestFlight-ready archive:

```bash
make archive             # → dist/Actnet.xcarchive (Release config, signed)
open dist/Actnet.xcarchive  # opens in Xcode Organizer
```

You must be signed into Xcode with an Apple ID that has access to our Xcode team. The `open` command will bring up Xcode Organizer:

1. Select the new archive, it should be at the top
2. **Distribute App** → App Store Connect → Upload.

After upload and a few minutes of processing, the build appears in App Store Connect. (https://appstoreconnect.apple.com) You'll want to sign in and add 'What to Test' and submit the build for testing.

## 5. Android app (manual)

The Android app version is also derived from Git, as above.

Run: `make android-release`. This requires the Android release signing key available to you in 1password.

It outputs to `mobile/android/app/build/outputs/apk/release/app-release.apk`. 

You can then upload it to Github using:

```bash
gh release upload v0.2.0 mobile/android/app/build/outputs/apk/release/app-release.apk
```

Currently the only way Android users can see a new release is by seeing it on the website, so you'll also want to update the website to point to the new release. (content/getting-started/sideload-android.md)

## 6. Web

The website is hosted at theavalanche.net and is in the `web/` folder. To build
and deploy it to Cloudflare, run `npm run deploy` in `web/` (it rebuilds with
Hugo first; a bare `wrangler deploy` ships whatever stale build is on disk).

Each release, bump the download links once the release is published: the
Android APK link in `content/getting-started/sideload-android.md` and
`desktopVersion` in `hugo.toml` (Desktop).