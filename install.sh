#!/usr/bin/env bash
set -euo pipefail

REPO="dpkay-io/gitreg"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-musl"  ;;
      aarch64) TARGET="aarch64-unknown-linux-musl"  ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    EXT="tar.gz"
    BINARY="gitreg"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin"  ;;
      arm64)  TARGET="aarch64-apple-darwin" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    EXT="tar.gz"
    BINARY="gitreg"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    case "$ARCH" in
      x86_64) TARGET="x86_64-pc-windows-msvc" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    EXT="zip"
    BINARY="gitreg.exe"
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

if [ -n "${GITREG_VERSION:-}" ]; then
  URL="https://github.com/${REPO}/releases/download/${GITREG_VERSION}/gitreg-${TARGET}.${EXT}"
else
  URL="https://github.com/${REPO}/releases/latest/download/gitreg-${TARGET}.${EXT}"
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading gitreg for ${TARGET}..."
DOWNLOADER=""
if command -v curl >/dev/null 2>&1; then
  DOWNLOADER="curl"
  curl -sSfL "$URL" -o "$TMP/archive.${EXT}"
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER="wget"
  wget -q "$URL" -O "$TMP/archive.${EXT}"
else
  echo "Error: curl or wget is required" >&2
  exit 1
fi

SHA256_URL="${URL}.sha256"
if [ "$DOWNLOADER" = "curl" ]; then
  curl -sSfL "$SHA256_URL" -o "$TMP/archive.sha256"
else
  wget -q "$SHA256_URL" -O "$TMP/archive.sha256"
fi

EXPECTED_HASH="$(awk '{print $1}' "$TMP/archive.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_HASH="$(sha256sum "$TMP/archive.${EXT}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_HASH="$(shasum -a 256 "$TMP/archive.${EXT}" | awk '{print $1}')"
else
  echo "Error: sha256sum or shasum not found — cannot verify download integrity" >&2
  exit 1
fi

if [ "$ACTUAL_HASH" != "$EXPECTED_HASH" ]; then
  echo "Error: SHA256 mismatch — archive may be corrupted or tampered with" >&2
  echo "  Expected: $EXPECTED_HASH" >&2
  echo "  Actual:   $ACTUAL_HASH" >&2
  exit 1
fi
echo "SHA256 verified."

if [ "$EXT" = "tar.gz" ]; then
  tar -xzf "$TMP/archive.${EXT}" -C "$TMP"
else
  unzip -q "$TMP/archive.${EXT}" -d "$TMP"
fi

if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

install -m 755 "$TMP/$BINARY" "$INSTALL_DIR/$BINARY"

PATH_NOTE=false
if [ "$INSTALL_DIR" = "$HOME/.local/bin" ]; then
  case ":${PATH}:" in
    *":$HOME/.local/bin:"*) ;;
    *) PATH_NOTE=true ;;
  esac
fi

export PATH="$INSTALL_DIR:$PATH"

if $PATH_NOTE; then
  echo ""
  echo "Note: Add \$HOME/.local/bin to your PATH permanently (e.g. in ~/.bashrc):"
  echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

gitreg init

echo ""
gitreg autoscan
echo ""
echo "gitreg is installed and ready."
