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
#
# Also available: `make bindings` (regenerate UniFFI Swift/Kotlin glue only,
# no xcframework or xcodebuild), `make dev` (homeserver only), `make check`,
# `make clippy`, `make fmt`, `make testbot`, `make relay`.
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

.PHONY: test test-server test-core test-e2e check clippy fmt ci db-up db-down migrate ios xcode archive ipa bindings dev testbot relay relay-release server-release dev-all node node-debug adminbot adminbot-bootstrap

# ----------------------------------------------------------------------------
# Node bindings (napi-rs)
# ----------------------------------------------------------------------------

node:
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build

node-debug:
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build:debug

# ----------------------------------------------------------------------------
# Adminbot (Node)
# ----------------------------------------------------------------------------
#
# Two-step bootstrap when first wiring adminbot onto a server:
#
#   1. `make adminbot-bootstrap` → registers, persists ~/.adminbot/state.json,
#      prints the assigned `did:local:...` DID, and exits.
#   2. Set ADMINBOT_DID=<that DID> in your server env (or .env) and restart
#      the homeserver. Now /v1/admin/ping accepts adminbot.
#   3. `make adminbot` → runs the bot continuously.
#
# Required env: ADMINBOT_SERVER_URL. Optional: ADMINBOT_INITIAL_ADMINS,
# ADMINBOT_STATE_DIR (default ./adminbot-state), ADMINBOT_DB_KEY,
# ADMINBOT_LOG (default info).

# Build the adminbot binary (and its dependency, @actnet/app-core if needed).
adminbot-build:
	cd node && [ -d node_modules ] || npm install
	cd node && [ -f packages/app-core/dist/index.js ] || npm run build -w @actnet/app-core
	cd node && npm run build -w @actnet/adminbot

# Bootstrap or continue an already-bootstrapped adminbot process. Behavior is
# the same target either way — adminbot detects via the state.json sidecar.
adminbot adminbot-bootstrap: adminbot-build
	cd node && ADMINBOT_SERVER_URL=$${ADMINBOT_SERVER_URL:-http://localhost:3000} \
		node packages/adminbot/dist/index.js

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

dev:
	cd core && ACTNET_ALLOW_DEV_DB=1 ACTNET_DISABLE_IP_RATE_LIMITS=1 RUST_LOG=tower_http=debug,server=debug cargo run -p server

db-up:
	docker compose -f infra/docker-compose.yml up -d --wait
	$(MAKE) migrate

db-down:
	docker compose -f infra/docker-compose.yml down

# Apply embedded schema migrations against the dev Postgres. Idempotent —
# safe to re-run. Same code path the prod release uses.
migrate:
	cd core && DATABASE_URL=$(TEST_DATABASE_URL) cargo run -q -p server -- migrate

testbot:
	cd core && RUST_LOG=actnet_testbot=debug,app_core=debug,tower_http=debug cargo run -p testbot

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

dev-all:
	python3 dev.py

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
		| (xcbeautify 2>/dev/null || cat)
	@echo "Archive: $(ARCHIVE_PATH)"

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
	cd mobile/ios/Actnet && set -a; [ -f $(PWD)/.env ] && . $(PWD)/.env; set +a; xcodegen generate
	@# Mark generated plists so they're obviously not hand-editable.
	@# Info.{Debug,Release}.plist are hand-maintained — only the entitlements
	@# file is generated by xcodegen.
	@f=mobile/ios/Actnet/Actnet.entitlements; \
	if [ -f "$$f" ]; then \
		python3 -c "p=open('$$f').read(); c='<!-- AUTO-GENERATED by xcodegen from project.yml -- DO NOT EDIT -->\n'; p2=p.replace(c,''); open('$$f','w').write(p2.replace('<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n','<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n'+c,1))"; \
	fi
