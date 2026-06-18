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

To re-cut a tag (e.g. you forgot to push the branch first):

```bash
git tag -d v0.2.0 && git push origin :refs/tags/v0.2.0   # delete local + remote
git tag v0.2.0    && git push origin v0.2.0              # re-tag and push
```

## 2. Server-side artifacts (automatic, via GitHub Actions)

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds every
first-party binary for both Linux arches (`x86_64` and `aarch64`) and attaches
them to a **draft** GitHub Release:

| Asset (per arch)                  | Contents                                  |
| --------------------------------- | ----------------------------------------- |
| `av-server-<target>.tar.gz`       | `avalanche-server` binary                 |
| `av-relay-<target>.tar.gz`        | `relay` binary                            |
| `av-adminbot-<target>.tar.gz`     | self-contained Node bot (node_modules + built packages) |
| `av-testbot-<target>.tar.gz`      | self-contained Node bot                   |

Then:

1. Watch the run: `gh run watch` (or the Actions tab on GitHub).
2. When it finishes, open the **draft release** on GitHub, add release notes.
3. **Publish** the draft to make the asset download URLs live, e.g.
   `https://github.com/lincolnq/avalanche/releases/download/v0.2.0/av-server-x86_64-unknown-linux-gnu.tar.gz` and mark it pre-release.


## 3. Mobile app (manual, from your Mac)

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
2. **Distribute App** → Internal Testing (or App Store Connect) → Upload.

After upload, the build appears in App Store Connect → TestFlight after
processing. (https://appstoreconnect.apple.com) TestFlight builds automatically go out to internal testers.

## 4. Web

The website is hosted at theavalanche.net and is in the `web/` folder. To build, run `hugo build` and then `wrangler deploy` to deploy it to Cloudflare.