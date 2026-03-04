# Operator Jack — Build Automation
# Usage: make [target]

VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
RUST_BIN := operator-jack
SWIFT_BIN := operator-macos-helper
INSTALL_DIR := /usr/local/bin

.PHONY: all build build-release test lint clean install uninstall universal help

## Default: build debug binaries
all: build

## Build debug binaries (Rust + Swift)
build:
	cargo build
	cd macos-helper && swift build

## Build release binaries (Rust + Swift)
build-release:
	cargo build --release
	cd macos-helper && swift build -c release

## Run all tests
test:
	cargo test

## Run lints (clippy + format check)
lint:
	cargo fmt --check
	cargo clippy -- -D warnings

## Clean build artifacts
clean:
	cargo clean
	cd macos-helper && swift package clean

## Install both binaries to /usr/local/bin
install: build-release
	@echo "Installing to $(INSTALL_DIR)..."
	install -d $(INSTALL_DIR)
	install -m 755 target/release/$(RUST_BIN) $(INSTALL_DIR)/$(RUST_BIN)
	install -m 755 macos-helper/.build/release/$(SWIFT_BIN) $(INSTALL_DIR)/$(SWIFT_BIN)
	@echo "Installed $(RUST_BIN) and $(SWIFT_BIN) to $(INSTALL_DIR)"

## Remove both binaries from /usr/local/bin
uninstall:
	rm -f $(INSTALL_DIR)/$(RUST_BIN)
	rm -f $(INSTALL_DIR)/$(SWIFT_BIN)
	@echo "Removed $(RUST_BIN) and $(SWIFT_BIN) from $(INSTALL_DIR)"

## Build universal macOS binaries (arm64 + x86_64) and package as tarball
universal:
	@echo "Building universal binaries for v$(VERSION)..."
	# Rust: build for both architectures
	rustup target add aarch64-apple-darwin x86_64-apple-darwin 2>/dev/null || true
	cargo build --release --target aarch64-apple-darwin
	cargo build --release --target x86_64-apple-darwin
	# Rust: create universal binary via lipo
	mkdir -p dist
	lipo -create \
		target/aarch64-apple-darwin/release/$(RUST_BIN) \
		target/x86_64-apple-darwin/release/$(RUST_BIN) \
		-output dist/$(RUST_BIN)
	# Swift: build for both architectures
	cd macos-helper && swift build -c release --arch arm64 --arch x86_64
	cp macos-helper/.build/apple/Products/Release/$(SWIFT_BIN) dist/$(SWIFT_BIN)
	# Package
	cd dist && tar czf $(RUST_BIN)-v$(VERSION)-macos-universal.tar.gz $(RUST_BIN) $(SWIFT_BIN)
	cd dist && shasum -a 256 $(RUST_BIN)-v$(VERSION)-macos-universal.tar.gz > $(RUST_BIN)-v$(VERSION)-macos-universal.tar.gz.sha256
	@echo "Built: dist/$(RUST_BIN)-v$(VERSION)-macos-universal.tar.gz"

## Show available targets
help:
	@echo "Operator Jack v$(VERSION)"
	@echo ""
	@echo "Targets:"
	@echo "  build          Build debug binaries (Rust + Swift)"
	@echo "  build-release  Build release binaries"
	@echo "  test           Run all Rust tests"
	@echo "  lint           Run clippy and format check"
	@echo "  clean          Remove build artifacts"
	@echo "  install        Install release binaries to $(INSTALL_DIR)"
	@echo "  uninstall      Remove binaries from $(INSTALL_DIR)"
	@echo "  universal      Build universal macOS binaries + tarball"
	@echo "  help           Show this help"
