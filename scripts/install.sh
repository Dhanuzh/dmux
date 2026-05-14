#!/usr/bin/env sh
set -eu

REPO="${DMUX_REPO:-}"
VERSION="${DMUX_VERSION:-latest}"
INSTALL_DIR="${DMUX_INSTALL_DIR:-$HOME/.local/bin}"

if [ -z "$REPO" ]; then
  if command -v git >/dev/null 2>&1; then
    ORIGIN_URL="$(git config --get remote.origin.url 2>/dev/null || true)"
    case "$ORIGIN_URL" in
      https://github.com/*)
        REPO="$(printf '%s' "$ORIGIN_URL" | sed -E 's#https://github.com/([^/]+/[^/.]+)(\.git)?#\1#')"
        ;;
      git@github.com:*)
        REPO="$(printf '%s' "$ORIGIN_URL" | sed -E 's#git@github.com:([^/]+/[^/.]+)(\.git)?#\1#')"
        ;;
      *)
        REPO=""
        ;;
    esac
  fi
fi

if [ -z "$REPO" ]; then
  echo "DMUX_REPO is not set and could not infer GitHub repo from git remote." >&2
  echo "Set DMUX_REPO like: export DMUX_REPO=owner/repo" >&2
  exit 1
fi

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64) TARGET="x86_64-unknown-linux-musl" ;;
      *)
        echo "Unsupported Linux architecture for prebuilt dmux: $ARCH" >&2
        exit 1
        ;;
    esac
    BIN_NAME="dmux"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *)
        echo "Unsupported macOS architecture for prebuilt dmux: $ARCH" >&2
        exit 1
        ;;
    esac
    BIN_NAME="dmux"
    ;;
  *)
    echo "Unsupported OS for this installer: $OS" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  RELEASE_JSON="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")"
  VERSION="$(printf '%s' "$RELEASE_JSON" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [ -z "$VERSION" ]; then
    echo "Could not resolve latest release tag for $REPO" >&2
    exit 1
  fi
fi

ASSET="dmux-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "Downloading $URL"
curl -fL "$URL" -o "$TMP_DIR/$ASSET"
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

echo "Installed $BIN_NAME to $INSTALL_DIR/$BIN_NAME"
echo "Ensure $INSTALL_DIR is in your PATH."
