# linkctl — build helpers. All real work is done by cargo.

CARGO   ?= cargo
BIN_DIR ?= bin
BINARY  := $(BIN_DIR)/linkctl
PREFIX  ?= $(HOME)/.local
VERSION ?= $(shell grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')
# Static musl target used for release artifacts; ARCH is the package name
# convention (amd64|arm64) used by the release workflow and mise.
TARGET  ?= $(shell uname -m | sed -e 's/x86_64/x86_64-unknown-linux-musl/' -e 's/aarch64/aarch64-unknown-linux-musl/')
ARCH    ?= $(shell uname -m | sed -e 's/x86_64/amd64/' -e 's/aarch64/arm64/')

.PHONY: all build debug test lint check install uninstall clean dist release

all: build

## Build the optimised binary into bin/linkctl
build:
	$(CARGO) build --release
	mkdir -p $(BIN_DIR)
	cp target/release/linkctl $(BINARY)
	@echo "built $(BINARY)"

## Build an unoptimised binary into bin/linkctl (faster to compile)
debug:
	$(CARGO) build
	mkdir -p $(BIN_DIR)
	cp target/debug/linkctl $(BINARY)
	@echo "built $(BINARY) (debug)"

## Run unit tests (never touches the camera)
test:
	$(CARGO) test

## Formatting and clippy, as run in CI
lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets --all-features -- -D warnings

## lint + test
check: lint test

## Install into $(PREFIX)/bin (default: ~/.local/bin)
install: build
	install -Dm755 $(BINARY) $(PREFIX)/bin/linkctl

uninstall:
	rm -f $(PREFIX)/bin/linkctl

## Build the static release binary for this machine and package it into
## dist/ (tar.gz always; deb/rpm/pkg.tar.zst when nfpm is installed).
## Same artifacts the release workflow publishes.
dist:
	rustup target add $(TARGET)
	$(CARGO) build --release --locked --target $(TARGET)
	scripts/package.sh target/$(TARGET)/release/linkctl $(VERSION) $(ARCH)

## Interactive release: propose the next semver tag, bump Cargo.toml,
## commit, tag and push. The tag triggers .github/workflows/release.yml.
release:
	@scripts/release.sh

clean:
	$(CARGO) clean
	rm -rf $(BIN_DIR) dist
