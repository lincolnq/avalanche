# ============================================================================
# Build & test commands for the actnet monorepo
#
# How it works
# ------------
# Rust code lives under `core/` (Cargo workspace). The iOS app lives under
# `mobile/ios/Actnet/` and consumes the Rust core through a UniFFI-generated
# Swift binding plus an `AppCoreFFI.xcframework`.
#
# iOS build chain (file-based incremental dependencies — make only redoes the
# steps whose inputs actually changed):
#
#   Rust sources ── UniFFI bindings ──┐
#                                     ├── xcframework ──┐
#   project.yml + Swift sources ──── xcodeproj ─────────┴── xcodebuild
#
# A no-op `make ios` is ~1s; touching only Swift skips the Rust rebuilds; only
# editing Rust triggers binding+xcframework regen.
#
# Common cases
# ------------
#   make ios             # build the iOS app for the simulator (most common)
#   make xcode           # prepare bindings + xcframework + xcodeproj for the
#                        # already-open Xcode (no xcodebuild)
#   make archive         # build a Release .xcarchive for TestFlight
#   make ipa             # export an uploadable .ipa from the archive
#   make dev-all         # run homeserver + testbot together (preferred).
#                        # Run the relay separately via `make relay`;
#                        # set RELAY_URL in .env so the server can reach it.
#   make test            # Rust unit tests (crypto + store + types + server)
#   make test-e2e        # app-core integration tests (needs a running server)
#   make db-up / db-down # start/stop Postgres in docker-compose
#   make db-reset        # wipe Postgres volume + dev-state/, re-migrate
#
# Also available: `make bindings` (regenerate UniFFI Swift/Kotlin glue only,
# no xcframework or xcodebuild), `make dev` (homeserver only), `make check`,
# `make clippy`, `make fmt`, `make node-testbot`, `make relay`.
# ============================================================================

TEST_DATABASE_URL ?= postgres://actnet:actnet-dev@localhost/actnet

# Input file globs used for incremental rebuild dependency tracking.
RUST_SOURCES := $(shell find core/crates -name '*.rs' -not -path '*/target/*' 2>/dev/null) \
                $(shell find core/crates -name 'Cargo.toml' 2>/dev/null) \
                core/Cargo.toml core/Cargo.lock
SWIFT_SOURCES := $(shell find mobile/ios/Actnet/Sources -name '*.swift' 2>/dev/null)

# Generated artifacts. These are real file targets, not phony — make uses their
# mtimes to decide what's stale.
SWIFT_BINDING := mobile/ios/Generated/app_core.swift
XCFRAMEWORK_STAMP := mobile/ios/AppCoreFFI.xcframework/Info.plist
XCODE_PROJ_FILE := mobile/ios/Actnet/Actnet.xcodeproj/project.pbxproj

# Android: UniFFI Kotlin glue + cross-compiled native libs (one .so per ABI).
# The arm64-v8a .so doubles as the make stamp for "native libs are current".
KOTLIN_BINDING := mobile/android/Generated/uniffi/app_core/app_core.kt
ANDROID_JNILIBS := mobile/android/app/src/main/jniLibs
ANDROID_SO_STAMP := $(ANDROID_JNILIBS)/arm64-v8a/libapp_core.so
# Gradle APK outputs, copied into dist/ by the android* targets (mirrors how the
# iOS archive/ipa land in dist/).
ANDROID_DEBUG_APK := mobile/android/app/build/outputs/apk/debug/app-debug.apk
ANDROID_RELEASE_APK := mobile/android/app/build/outputs/apk/release/app-release.apk
ANDROID_ABIS := arm64-v8a x86_64
# minSdk in app/build.gradle.kts — the native API level to compile against.
ANDROID_API := 26
# cargo-ndk and openssl-src both locate the toolchain via ANDROID_NDK_HOME.
# Default to the highest NDK installed under the SDK; override from the env.
ANDROID_HOME ?= $(HOME)/Library/Android/sdk
ANDROID_NDK_HOME ?= $(shell ls -d $(ANDROID_HOME)/ndk/* 2>/dev/null | sort -V | tail -1)
# Gradle needs a JDK 17+. Use $JAVA_HOME if set, else fall back to the JBR that
# ships with Android Studio (macOS). Override from the env if neither fits.
ANDROID_JAVA_HOME ?= $(or $(JAVA_HOME),/Applications/Android Studio.app/Contents/jbr/Contents/Home)

# App version, derived from git so it never has to be hand-edited in
# project.yml. MARKETING_VERSION (CFBundleShortVersionString) is the latest
# reachable tag with any leading `v` stripped — the same bare semver the
# server release uses, so app and server share one version scheme.
# CURRENT_PROJECT_VERSION (CFBundleVersion — the build number App Store Connect
# requires to strictly increase on every upload) is the total commit count,
# which climbs monotonically. Both are overridable from the environment
# (e.g. CI) via ?=. Passed to xcodebuild as command-line build-setting
# overrides below, so they're always current regardless of whether make
# decided the .xcodeproj was stale. The android target hands the same two
# variables to Gradle as -P properties (-> versionName / versionCode), so iOS
# and Android always stamp identical version + build numbers.
GIT_TAG := $(shell git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//')
MARKETING_VERSION ?= $(or $(GIT_TAG),0.0.0)
CURRENT_PROJECT_VERSION ?= $(or $(shell git rev-list --count HEAD 2>/dev/null),0)

# Node @actnet/app-core napi binding — same file-based incremental approach as
# the iOS chain. The native build regenerates native/index.d.ts every run, so
# it doubles as the stamp for "the binding is current with the Rust sources".
# The TS wrapper compiles to dist/index.js. Both are real file targets gated on
# their inputs, so a no-op bot build skips the expensive napi rebuild.
APP_CORE_TS_SOURCES := $(shell find node/packages/app-core/src -name '*.ts' 2>/dev/null)
APP_CORE_NATIVE := node/packages/app-core/native/index.d.ts
APP_CORE_DIST := node/packages/app-core/dist/index.js

.PHONY: test test-server test-core test-e2e check clippy fmt ci db-up db-down db-reset migrate ios xcode archive ipa bindings android android-release android-minify-test android-bindings dev relay relay-release server-release dev-all dev-desktop node node-debug node-app-core node-adminbot node-adminbot-build node-testbot node-testbot-build desktop

# ----------------------------------------------------------------------------
# Node bindings (napi-rs)
# ----------------------------------------------------------------------------

node:
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build

node-debug:
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build:debug

# Rebuild the @actnet/app-core napi native binding when any Rust source
# changes (the binding statically links the whole core, so the dep set is the
# same RUST_SOURCES the xcframework uses). cargo's incremental keeps this cheap.
$(APP_CORE_NATIVE): $(RUST_SOURCES)
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build:native -w @actnet/app-core

# Recompile the TS wrapper when the native binding or the wrapper sources change.
$(APP_CORE_DIST): $(APP_CORE_NATIVE) $(APP_CORE_TS_SOURCES)
	cd node && npm run build:ts -w @actnet/app-core

# Human-friendly alias for "bring the shared binding up to date". The real
# gating lives on the file targets above; this just names them.
node-app-core: $(APP_CORE_DIST)

# ----------------------------------------------------------------------------
# Adminbot (Node)
# ----------------------------------------------------------------------------
#
# The adminbot registers under the reserved DID `did:local:adminbot`, which
# is the server's default value of ADMINBOT_DID. No two-step bootstrap is
# required — first launch registers, subsequent launches re-login.
#
# Required env: ADMINBOT_SERVER_URL. Optional: ADMINBOT_INITIAL_ADMINS,
# ADMINBOT_STATE_DIR (default ./adminbot-state), ADMINBOT_DB_KEY,
# ADMINBOT_LOG (default info).

# Build the adminbot package. Depends on the shared app-core binding (which
# only rebuilds when the Rust/TS sources actually change).
node-adminbot-build: $(APP_CORE_DIST)
	cd node && npm run build -w @actnet/adminbot

# Run the adminbot. Idempotent — first run registers the reserved DID, later
# runs re-login against the existing SQLCipher store.
node-adminbot: node-adminbot-build
	cd node && ADMINBOT_SERVER_URL=$${ADMINBOT_SERVER_URL:-http://localhost:3000} \
		node packages/adminbot/dist/index.js

# ----------------------------------------------------------------------------
# Testbot (Node)
# ----------------------------------------------------------------------------
#
# A standalone HTTP service that spins up ephemeral AI chatbot accounts on
# demand (see node/packages/testbot). Replaced the old Rust `testbot` crate.
#
# Required env: none (HOMESERVER_URL defaults to http://localhost:3000).
# Optional: ANTHROPIC_API_KEY (else bots echo), TESTBOT_BIND_ADDR
# (default 0.0.0.0:3001), TESTBOT_LOG (default info).

# Build the testbot package. Depends on the shared app-core binding (which
# only rebuilds when the Rust/TS sources actually change).
node-testbot-build: $(APP_CORE_DIST)
	cd node && npm run build -w @actnet/testbot

# Run the testbot HTTP service.
node-testbot: node-testbot-build
	cd node && HOMESERVER_URL=$${HOMESERVER_URL:-http://localhost:3000} \
		node packages/testbot/dist/index.js

# ----------------------------------------------------------------------------
# Rust
# ----------------------------------------------------------------------------

test: test-core test-server

test-e2e:
	cd core && cargo test -p app-core

test-core:
	cd core && cargo test -p crypto -p store -p types

test-server:
	cd core && TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test -p server

check:
	cd core && cargo check

clippy:
	cd core && cargo clippy

fmt:
	cd core && cargo fmt

ci: check clippy test-server
	@echo "All checks passed."

# ----------------------------------------------------------------------------
# Local services
# ----------------------------------------------------------------------------

# Attachment blobs (docs/35) live under the repo-root dev-state/ tree on dev
# machines (gitignored, wiped by `make db-reset`), alongside the bots' SQLCipher
# stores. The path is absolute because the server runs with cwd=core/. We
# override here because the server's default targets the production state dir
# (/var/lib/avalanche/attachments), which doesn't exist on a dev box.
dev:
	cd core && ACTNET_ALLOW_DEV_DB=1 ACTNET_DISABLE_IP_RATE_LIMITS=1 REGISTRATION_SHARED_SECRET=$(or $(REGISTRATION_SHARED_SECRET),CHANGEME) ATTACHMENT_BLOB_DIR=$(CURDIR)/dev-state/attachments RUST_LOG=tower_http=debug,server=debug cargo run -p server

db-up:
	docker compose -f infra/docker-compose.yml up -d --wait
	$(MAKE) migrate

db-down:
	docker compose -f infra/docker-compose.yml down

# Wipe the dev Postgres volume AND the local bot state in lockstep, then bring
# the DB back up with fresh migrations. The two must reset together: the
# adminbot/testbot keep their own SQLCipher store under dev-state/, and a
# server-DB-only reset leaves those bots holding an account whose server-side
# device row is gone — re-login then 404s on /v1/auth/challenge and the WS
# never connects. Run this instead of dropping Postgres by hand.
db-reset:
	docker compose -f infra/docker-compose.yml down -v
	rm -rf dev-state
	$(MAKE) db-up

# Apply embedded schema migrations against the dev Postgres. Idempotent —
# safe to re-run. Same code path the prod release uses.
migrate:
	cd core && DATABASE_URL=$(TEST_DATABASE_URL) cargo run -q -p server -- migrate

relay:
	cd core && RUST_LOG=relay=debug,tower_http=debug cargo run -p relay

# Build a portable Linux x86_64 release binary in Docker so it'll run on
# any modern Debian/Ubuntu droplet (links against the host's libssl/libcrypto).
# Output: dist/relay
relay-release:
	@command -v docker >/dev/null || { echo "Docker required for relay-release"; exit 1; }
	@mkdir -p dist
	docker run --rm --platform linux/amd64 \
	  -v "$(PWD)":/src -w /src/core \
	  -e CARGO_TARGET_DIR=/src/dist/cargo-target \
	  rust:1-bookworm \
	  bash -c "apt-get update -qq && apt-get install -y -qq libssl-dev pkg-config && cargo build --release -p relay"
	cp dist/cargo-target/release/relay dist/relay
	@strip dist/relay 2>/dev/null || true
	@ls -lh dist/relay
	@echo "Built dist/relay — copy to droplet with scp."

# Build a portable Linux x86_64 release server binary in Docker.
# Output: dist/avalanche-server
server-release:
	@command -v docker >/dev/null || { echo "Docker required for server-release"; exit 1; }
	@mkdir -p dist
	docker run --rm --platform linux/amd64 \
	  -v "$(PWD)":/src -w /src/core \
	  -e CARGO_TARGET_DIR=/src/dist/cargo-target \
	  rust:1-bookworm \
	  bash -c "apt-get update -qq && apt-get install -y -qq libssl-dev pkg-config cmake protobuf-compiler && cargo build --release -p server"
	cp dist/cargo-target/release/avalanche-server dist/avalanche-server
	@strip dist/avalanche-server 2>/dev/null || true
	@ls -lh dist/avalanche-server
	@echo "Built dist/avalanche-server — copy to droplet with scp."

desktop:
	cd desktop && npm run tauri dev

desktop-bindings:
	cd desktop/src-tauri && cargo run --features codegen

dev-all:
	python3 dev.py

dev-desktop:
	python3 dev.py & \
	DEV_PID=$$!; \
	trap 'kill $$DEV_PID 2>/dev/null' EXIT; \
	cd desktop && npm run tauri dev

dev-invite:
	@python3 dev-invite.py

# ----------------------------------------------------------------------------
# iOS — see the build-chain diagram at the top of this file.
# ----------------------------------------------------------------------------

# Prepare everything Xcode needs to build the app itself (bindings,
# xcframework, regenerated Xcode project) but stop short of running
# xcodebuild. Use this when you have Xcode open and want to click Run there.
xcode: $(XCFRAMEWORK_STAMP) $(XCODE_PROJ_FILE)

# Build the iOS app for the simulator. Brings every input up to date and then
# runs xcodebuild (which has its own incremental compilation for Swift).
ios: $(XCFRAMEWORK_STAMP) $(XCODE_PROJ_FILE) $(SWIFT_SOURCES)
	set -o pipefail; xcodebuild \
		-project mobile/ios/Actnet/Actnet.xcodeproj \
		-scheme Actnet \
		-destination 'generic/platform=iOS Simulator' \
		-configuration Debug \
		build \
		CODE_SIGNING_ALLOWED=NO \
		MARKETING_VERSION=$(MARKETING_VERSION) \
		CURRENT_PROJECT_VERSION=$(CURRENT_PROJECT_VERSION) \
		| (xcbeautify 2>/dev/null || cat)

# ----------------------------------------------------------------------------
# iOS release / TestFlight
# ----------------------------------------------------------------------------
#
#   make archive    # produces dist/Actnet.xcarchive (Release config, signed)
#   make ipa        # produces dist/Actnet.ipa from the archive
#
# Then upload to App Store Connect via one of:
#   - Xcode → Window → Organizer → select archive → Distribute App
#   - open the Transporter.app and drop in dist/Actnet.ipa
#   - xcrun altool --upload-app -f dist/Actnet.ipa -t ios \
#         --apiKey <key-id> --apiIssuer <issuer-id>
#
# Release config (see project.yml) sets aps-environment=production, which is
# required for TestFlight / App Store builds — sandbox APNs is dev-only.
# Code signing uses automatic signing with team 7FVK3RR3TV; sign into Xcode
# with an Apple ID that has access to the team at least once before archiving.

ARCHIVE_PATH := dist/Actnet.xcarchive
IPA_DIR := dist

# Build a TestFlight-ready archive (Release, real-device slice, signed).
archive: $(XCFRAMEWORK_STAMP) $(XCODE_PROJ_FILE) $(SWIFT_SOURCES)
	@mkdir -p dist
	set -o pipefail; xcodebuild \
		-project mobile/ios/Actnet/Actnet.xcodeproj \
		-scheme Actnet \
		-destination 'generic/platform=iOS' \
		-configuration Release \
		-archivePath $(ARCHIVE_PATH) \
		archive \
		MARKETING_VERSION=$(MARKETING_VERSION) \
		CURRENT_PROJECT_VERSION=$(CURRENT_PROJECT_VERSION) \
		| (xcbeautify 2>/dev/null || cat)
	@echo "Archive: $(ARCHIVE_PATH) ($(MARKETING_VERSION) build $(CURRENT_PROJECT_VERSION))"

# Export the archive as an .ipa using mobile/ios/ExportOptions.plist.
ipa: $(ARCHIVE_PATH)
	set -o pipefail; xcodebuild \
		-exportArchive \
		-archivePath $(ARCHIVE_PATH) \
		-exportPath $(IPA_DIR) \
		-exportOptionsPlist mobile/ios/ExportOptions.plist \
		| (xcbeautify 2>/dev/null || cat)
	@ls -lh $(IPA_DIR)/*.ipa
	@echo "Upload via Xcode Organizer, Transporter.app, or altool."

$(ARCHIVE_PATH):
	@echo "No archive found at $(ARCHIVE_PATH) — run \`make archive\` first." >&2
	@exit 1

# Regenerate just the UniFFI bindings (Swift + Kotlin) — does NOT rebuild the
# xcframework or run xcodebuild. Useful when you only need updated Swift glue
# (e.g., to check FFI signatures compile against an existing xcframework).
bindings: $(SWIFT_BINDING)

# Regenerate UniFFI bindings (Swift + Kotlin) when Rust sources change.
$(SWIFT_BINDING): $(RUST_SOURCES)
	cd core && cargo build -p app-core
	cd core && cargo run -p app-core --bin uniffi-bindgen generate --library target/debug/libapp_core.dylib --language swift --out-dir ../mobile/ios/Generated
	cd core && cargo run -p app-core --bin uniffi-bindgen generate --library target/debug/libapp_core.dylib --language kotlin --no-format --out-dir ../mobile/android/Generated

# Rebuild AppCoreFFI.xcframework when Rust sources or the FFI header change.
# We use Info.plist inside the xcframework as a stamp file (xcframework itself
# is a directory, which make handles awkwardly).
$(XCFRAMEWORK_STAMP): $(RUST_SOURCES) $(SWIFT_BINDING)
	cd core && cargo build -p app-core --target aarch64-apple-ios --release
	cd core && cargo build -p app-core --target aarch64-apple-ios-sim --release
	mkdir -p mobile/ios/RustFramework/Headers
	cp mobile/ios/Generated/app_coreFFI.h mobile/ios/RustFramework/Headers/
	cp mobile/ios/Generated/app_coreFFI.modulemap mobile/ios/RustFramework/Headers/module.modulemap
	rm -rf mobile/ios/AppCoreFFI.xcframework
	xcodebuild -create-xcframework \
		-library core/target/aarch64-apple-ios/release/libapp_core.a \
		-headers mobile/ios/RustFramework/Headers \
		-library core/target/aarch64-apple-ios-sim/release/libapp_core.a \
		-headers mobile/ios/RustFramework/Headers \
		-output mobile/ios/AppCoreFFI.xcframework

# Regenerate the Xcode project when project.yml changes or when the Swift
# source tree gains/loses files (xcodegen globs Sources/).
$(XCODE_PROJ_FILE): mobile/ios/Actnet/project.yml $(SWIFT_SOURCES) $(wildcard .env)
	@# Source .env so RELAY_URL (and similar) reach xcodegen for ${VAR}
	@# substitution in project.yml. `set -a` auto-exports each var.
	@# .env is a wildcard prereq so changes to RELAY_URL trigger a
	@# regenerate (and if .env is absent, no prereq is added).
	cd mobile/ios/Actnet && set -a; \
		[ -f $(PWD)/.env ] && . $(PWD)/.env; \
		MARKETING_VERSION='$(MARKETING_VERSION)'; \
		CURRENT_PROJECT_VERSION='$(CURRENT_PROJECT_VERSION)'; \
		set +a; \
		xcodegen generate
	@# Mark generated plists so they're obviously not hand-editable.
	@# Info.{Debug,Release}.plist are hand-maintained — only the entitlements
	@# file is generated by xcodegen.
	@f=mobile/ios/Actnet/Actnet.entitlements; \
	if [ -f "$$f" ]; then \
		python3 -c "p=open('$$f').read(); c='<!-- AUTO-GENERATED by xcodegen from project.yml -- DO NOT EDIT -->\n'; p2=p.replace(c,''); open('$$f','w').write(p2.replace('<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n','<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n'+c,1))"; \
	fi

# ----------------------------------------------------------------------------
# Android — UniFFI Kotlin bindings + native libs (cargo-ndk).
# ----------------------------------------------------------------------------
#
# Two targets, mirroring the iOS split:
#   make android-bindings   # Kotlin glue + per-ABI native libs only (no Gradle).
#                           # The Android analog of `make xcode` — prepares
#                           # everything the app build consumes, minus the build.
#   make android            # the above, then Gradle builds the debug APK.
#
# Both are self-contained (no Xcode), so they also work on Linux/Windows.
# Prerequisites:
#   - rustup target add aarch64-linux-android x86_64-linux-android
#   - cargo install cargo-ndk
#   - an NDK under $ANDROID_HOME/ndk (Android Studio > SDK Manager > SDK Tools > NDK)
#
# The first build cross-compiles OpenSSL + SQLCipher + libsignal per ABI, so it
# is slow (several minutes); subsequent builds are incremental via cargo.

# Build the debug APK end to end. Brings bindings + native libs up to date, then
# runs Gradle (which has its own incremental Kotlin compilation).
android: android-bindings
	cd mobile/android && JAVA_HOME="$(ANDROID_JAVA_HOME)" ANDROID_HOME="$(ANDROID_HOME)" ./gradlew assembleDebug \
		-PMARKETING_VERSION=$(MARKETING_VERSION) \
		-PCURRENT_PROJECT_VERSION=$(CURRENT_PROJECT_VERSION)
	@mkdir -p dist
	@cp $(ANDROID_DEBUG_APK) dist/avalanche-debug.apk
	@ls -lh dist/avalanche-debug.apk

# Build the signed, distributable release APK (arm64-v8a only). Signing material
# is pulled from 1Password into a RAM disk + env vars at build time and torn down
# afterward — nothing touches persistent disk (see mobile/android/release-sign.sh).
# Requires the 1Password CLI signed in. Without valid credentials Gradle produces
# an unsigned APK that won't install.
# Output: mobile/android/app/build/outputs/apk/release/app-release.apk
android-release: android-bindings
	JAVA_HOME="$(ANDROID_JAVA_HOME)" ANDROID_HOME="$(ANDROID_HOME)" \
		MARKETING_VERSION="$(MARKETING_VERSION)" \
		CURRENT_PROJECT_VERSION="$(CURRENT_PROJECT_VERSION)" \
		mobile/android/release-sign.sh
	@mkdir -p dist
	@cp $(ANDROID_RELEASE_APK) dist/avalanche-release.apk
	@ls -lh dist/avalanche-release.apk

# Minified (R8 + resource-shrunk) release APK with the stripped .so, but signed
# with the DEBUG keystore (-PdebugSignRelease). Because it's debug-signed with the
# same applicationId, it installs straight over a `make android` debug build with
# no signature-mismatch reinstall — so you can test the shrunk build (and verify
# the R8 keep rules don't break the UniFFI/JNA native boundary at runtime) without
# losing app data/login. NOT for distribution — use `make android-release` for that.
# Output: dist/avalanche-minify-test.apk
android-minify-test: android-bindings
	cd mobile/android && JAVA_HOME="$(ANDROID_JAVA_HOME)" ANDROID_HOME="$(ANDROID_HOME)" ./gradlew assembleRelease \
		-PdebugSignRelease \
		-PMARKETING_VERSION=$(MARKETING_VERSION) \
		-PCURRENT_PROJECT_VERSION=$(CURRENT_PROJECT_VERSION)
	@mkdir -p dist
	@cp $(ANDROID_RELEASE_APK) dist/avalanche-minify-test.apk
	@ls -lh dist/avalanche-minify-test.apk
	@echo "Debug-signed minified APK — installs over your debug build: dist/avalanche-minify-test.apk"

android-bindings: $(KOTLIN_BINDING) $(ANDROID_SO_STAMP)

# Regenerate the Kotlin UniFFI glue from the host build of app-core.
$(KOTLIN_BINDING): $(RUST_SOURCES)
	cd core && cargo build -p app-core
	cd core && cargo run -p app-core --bin uniffi-bindgen generate --library target/debug/libapp_core.dylib --language kotlin --no-format --out-dir ../mobile/android/Generated

# Cross-compile app-core into one libapp_core.so per ABI. cargo-ndk drops each
# into $(ANDROID_JNILIBS)/<abi>/, which AGP packages automatically.
$(ANDROID_SO_STAMP): $(RUST_SOURCES)
	@command -v cargo-ndk >/dev/null || { echo "cargo-ndk not installed — run: cargo install cargo-ndk" >&2; exit 1; }
	@[ -n "$(ANDROID_NDK_HOME)" ] && [ -d "$(ANDROID_NDK_HOME)" ] || { echo "No NDK found under $(ANDROID_HOME)/ndk — install one in Android Studio (SDK Manager > SDK Tools > NDK) or set ANDROID_NDK_HOME" >&2; exit 1; }
	cd core && ANDROID_NDK_HOME="$(ANDROID_NDK_HOME)" cargo ndk \
		$(foreach abi,$(ANDROID_ABIS),-t $(abi)) \
		-P $(ANDROID_API) \
		-o ../$(ANDROID_JNILIBS) \
		build --release -p app-core
