#!/usr/bin/env bash
# Package a built linkctl binary for one Linux architecture.
#
# Usage: scripts/package.sh <binary> <version> <arch>
#   arch: amd64 | arm64
#
# Produces in dist/:
#   linkctl_<version>_linux_<arch>.tar.gz          (always)
#   linkctl_<version>_linux_<arch>.deb             (if nfpm is available)
#   linkctl_<version>_linux_<arch>.rpm             (if nfpm is available)
#   linkctl_<version>_linux_<arch>.pkg.tar.zst     (if nfpm is available)
#
# The tarball layout (binary + LICENSE + README at the root) is what the
# mise `github:`/`ubi` backends expect.

set -euo pipefail

binary="${1:?usage: package.sh <binary> <version> <arch>}"
version="${2:?usage: package.sh <binary> <version> <arch>}"
arch="${3:?usage: package.sh <binary> <version> <arch>}"

case "$arch" in
  amd64|arm64) ;;
  *) echo "error: arch must be amd64 or arm64 (got $arch)" >&2; exit 1 ;;
esac

cd "$(git rev-parse --show-toplevel)"
mkdir -p dist
base="linkctl_${version}_linux_${arch}"

# --- tar.gz -----------------------------------------------------------------
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -m755 "$binary" "$stage/linkctl"
install -m644 LICENSE README.md "$stage/"
mkdir -p "$stage/contrib"
install -m644 contrib/99-insta360-link.rules "$stage/contrib/"
tar -C "$stage" -czf "dist/${base}.tar.gz" linkctl LICENSE README.md contrib
echo "dist/${base}.tar.gz"

# --- deb / rpm / archlinux via nfpm ----------------------------------------
NFPM="${NFPM:-$(command -v nfpm || true)}"
if [[ -z "$NFPM" ]]; then
  echo "nfpm not found; skipping deb/rpm/archlinux packages" >&2
  exit 0
fi

# nfpm does not expand variables in `contents[].src`; stage at a fixed path.
mkdir -p dist/.stage
install -m755 "$binary" dist/.stage/linkctl
export VERSION="$version" ARCH="$arch"
for fmt in deb rpm archlinux; do
  case "$fmt" in
    deb) out="dist/${base}.deb" ;;
    rpm) out="dist/${base}.rpm" ;;
    archlinux) out="dist/${base}.pkg.tar.zst" ;;
  esac
  "$NFPM" package --config packaging/nfpm.yaml --packager "$fmt" --target "$out" >/dev/null
  echo "$out"
done
rm -rf dist/.stage
