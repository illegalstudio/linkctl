#!/usr/bin/env bash
# Generate the linkctl-bin PKGBUILD for a released version.
#
# Usage: scripts/gen-pkgbuild.sh <version> <sha256_amd64> <sha256_arm64> [source_base_url]
#
# Fills packaging/aur/linkctl-bin/PKGBUILD.in and prints the result. The
# release workflow commits the output to the AUR; developers can pass a
# file:// base URL to test the package with makepkg locally.

set -euo pipefail

version="${1:?usage: gen-pkgbuild.sh <version> <sha256_amd64> <sha256_arm64> [source_base_url]}"
sha_amd64="${2:?missing sha256 for amd64}"
sha_arm64="${3:?missing sha256 for arm64}"
base="${4:-https://github.com/illegalstudio/linkctl/releases/download/v${version}}"

cd "$(git rev-parse --show-toplevel)"
sed -e "s|@VERSION@|${version}|g" \
    -e "s|@SHA256_AMD64@|${sha_amd64}|g" \
    -e "s|@SHA256_ARM64@|${sha_arm64}|g" \
    -e "s|@SOURCE_BASE@|${base}|g" \
    packaging/aur/linkctl-bin/PKGBUILD.in
