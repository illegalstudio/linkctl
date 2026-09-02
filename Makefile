# linkctl — build helpers. All real work is done by cargo.

CARGO   ?= cargo
BIN_DIR ?= bin
BINARY  := $(BIN_DIR)/linkctl
PREFIX  ?= $(HOME)/.local

.PHONY: all build debug test lint check install uninstall clean

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

clean:
	$(CARGO) clean
	rm -rf $(BIN_DIR)
