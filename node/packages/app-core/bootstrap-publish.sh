#!/usr/bin/env bash
# One-time first-publish of @theavalanche/app-core and its four platform
# sub-packages, using native binaries built by CI (the `napi-build` job in
# .github/workflows/release.yml). Trusted publishing can't create a package, so
# the FIRST publish is done by hand from a machine logged into npm; every later
# release publishes from CI via OIDC. See node/CLAUDE.md "Publishing to npm".
#
# Prereqs:
#   - `npm login` (your own account, member of the @theavalanche org, 2FA ready)
#   - Trigger the workflow manually (Actions → Release → Run workflow) so the
#     napi-build job produces the three `napi-<triple>` artifacts, then download
#     them into one directory:
#       gh run download <run-id> --dir /tmp/napi
#
# Usage (run FROM node/packages/app-core — napi config lives here):
#   ./bootstrap-publish.sh <version> /tmp/napi
#   e.g. ./bootstrap-publish.sh 0.4.0 /tmp/napi   (note: NO leading "v")
set -euo pipefail

VERSION="${1:?usage: bootstrap-publish.sh <version> <downloaded-artifacts-dir>}"
ARTIFACTS="${2:?usage: bootstrap-publish.sh <version> <downloaded-artifacts-dir>}"

# Always operate from the package dir so napi reads THIS package.json's config.
cd "$(dirname "$0")"

if [ ! -d "$ARTIFACTS" ]; then
  echo "error: artifacts dir '$ARTIFACTS' not found" >&2
  exit 1
fi

# Collect the three platform .node files + the (target-independent) loader glue
# from the downloaded artifact tree into native/.
mkdir -p native
found=$(find "$ARTIFACTS" -name 'app-core.*.node' | wc -l | tr -d ' ')
if [ "$found" -ne 3 ]; then
  echo "error: expected 3 app-core.*.node files under '$ARTIFACTS', found $found" >&2
  echo "       (did all three napi-build matrix legs succeed and download?)" >&2
  exit 1
fi
find "$ARTIFACTS" -name 'app-core.*.node' -exec cp {} native/ \;
cp "$(find "$ARTIFACTS" -name index.js    | head -1)" native/index.js
cp "$(find "$ARTIFACTS" -name index.d.ts  | head -1)" native/index.d.ts
echo "staged binaries:"; ls -1 native/*.node

# Build the TypeScript wrapper (needs native/index.d.ts, staged above).
npm run build:ts

# Stamp the version, assemble the per-platform npm packages, publish them, then
# publish the main package. npm will prompt for your 2FA one-time code.
npm version "$VERSION" --no-git-tag-version --allow-same-version
npx napi create-npm-dir -t .   # -t is --target (output dir); "." -> ./npm/<triple>
npx napi artifacts -d native
npx napi prepublish -t npm --skip-gh-release   # publishes the 4 sub-packages
npm publish                                    # publishes @theavalanche/app-core

echo "Published @theavalanche/app-core@$VERSION + 3 platform packages."
echo "Next: add a Trusted Publisher to all four packages on npmjs.com, then"
echo "future stable tags publish from CI via OIDC (no token)."
