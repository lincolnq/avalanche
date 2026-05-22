TEST_DATABASE_URL ?= postgres://actnet:actnet-dev@localhost/actnet

.PHONY: test test-server test-core test-e2e check clippy fmt ci mobile-rebuild db-up db-down bindings ios dev testbot relay dev-all

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

mobile-rebuild:
	make bindings && make ios

dev:
	cd core && RUST_LOG=tower_http=debug,server=debug cargo run -p server

db-up:
	docker compose -f infra/docker-compose.yml up -d

testbot:
	cd core && RUST_LOG=actnet_testbot=debug,app_core=debug,tower_http=debug cargo run -p testbot

relay:
	cd core && RUST_LOG=relay=debug,tower_http=debug cargo run -p relay

db-down:
	docker compose -f infra/docker-compose.yml down

dev-all:
	python3 dev.py

ios: bindings ios-xcframework
	cd mobile/ios/Actnet && xcodegen generate

ios-xcframework:
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

bindings:
	cd core && cargo build -p app-core
	cd core && cargo run -p app-core --bin uniffi-bindgen generate --library target/debug/libapp_core.dylib --language swift --out-dir ../mobile/ios/Generated
	cd core && cargo run -p app-core --bin uniffi-bindgen generate --library target/debug/libapp_core.dylib --language kotlin --no-format --out-dir ../mobile/android/Generated
