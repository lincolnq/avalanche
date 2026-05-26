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
#   make dev-all         # run homeserver + testbot + relay together (preferred)
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

.PHONY: test test-server test-core test-e2e check clippy fmt ci db-up db-down ios xcode bindings dev testbot relay dev-all

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
	docker compose -f infra/docker-compose.yml up -d

db-down:
	docker compose -f infra/docker-compose.yml down

testbot:
	cd core && RUST_LOG=actnet_testbot=debug,app_core=debug,tower_http=debug cargo run -p testbot

relay:
	cd core && RUST_LOG=relay=debug,tower_http=debug cargo run -p relay

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
$(XCODE_PROJ_FILE): mobile/ios/Actnet/project.yml $(SWIFT_SOURCES)
	cd mobile/ios/Actnet && xcodegen generate
	@# Mark generated plists so they're obviously not hand-editable
	@for f in mobile/ios/Actnet/Actnet.entitlements mobile/ios/Actnet/Info.plist; do \
		if [ -f "$$f" ]; then \
			python3 -c "p=open('$$f').read(); c='<!-- AUTO-GENERATED by xcodegen from project.yml -- DO NOT EDIT -->\n'; p2=p.replace(c,''); open('$$f','w').write(p2.replace('<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n','<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n'+c,1))"; \
		fi; \
	done
