#!/bin/sh
set -eu

REPO="aloglu/pester"
BIN_NAME="pester"

say() {
  printf '%s\n' "$1"
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    say "Required command not found: $1"
    exit 1
  fi
}

checksum_entry() {
  artifact="$1"
  checksums="$2"

  while IFS= read -r line; do
    case "$line" in
      *"  ${artifact}")
        printf '%s\n' "$line"
        return 0
        ;;
    esac
  done < "$checksums"

  return 1
}

OS="${PESTER_INSTALL_OS:-$(uname -s)}"
ARCH="${PESTER_INSTALL_ARCH:-$(uname -m)}"

case "$OS" in
  Linux) IS_MACOS=0 ;;
  Darwin) IS_MACOS=1 ;;
  *)
    say "Unsupported OS: $OS"
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64 | amd64)
    if [ "$IS_MACOS" -eq 1 ]; then
      say "Intel macOS is not supported yet. Use an Apple Silicon Mac or a different platform."
      exit 1
    fi
    TARGET_ARCH="x86_64"
    ;;
  arm64 | aarch64) TARGET_ARCH="aarch64" ;;
  *)
    say "Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

if [ "$IS_MACOS" -eq 1 ]; then
  ARTIFACT="pester-macos-${TARGET_ARCH}.tar.gz"
else
  ARTIFACT="pester-linux-${TARGET_ARCH}.tar.gz"
fi

if [ "${PESTER_INSTALL_DRY_RUN:-0}" = "1" ]; then
  say "$ARTIFACT"
  exit 0
fi

need curl
need tar
need install

BASE_URL="https://github.com/${REPO}/releases/latest/download"
TMP_DIR="$(mktemp -d)"
INSTALL_DIR="${HOME}/.local/bin"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

say "Downloading ${ARTIFACT}..."
curl -fsSL "${BASE_URL}/${ARTIFACT}" -o "${TMP_DIR}/${ARTIFACT}"
curl -fsSL "${BASE_URL}/checksums.txt" -o "${TMP_DIR}/checksums.txt"

if ! checksum_entry "$ARTIFACT" "${TMP_DIR}/checksums.txt" > "${TMP_DIR}/checksum.txt"; then
  say "Checksum entry not found for ${ARTIFACT}."
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && sha256sum -c checksum.txt)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && shasum -a 256 -c checksum.txt)
else
  say "Could not verify checksum: sha256sum or shasum is required."
  exit 1
fi

mkdir -p "$INSTALL_DIR"
tar -xzf "${TMP_DIR}/${ARTIFACT}" -C "$TMP_DIR"
install -m 0755 "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

if [ "$IS_MACOS" -eq 1 ]; then
  APP_INSTALL_DIR="${HOME}/Applications"
  mkdir -p "$APP_INSTALL_DIR"
  rm -rf "${APP_INSTALL_DIR}/Pester.app"
  cp -R "${TMP_DIR}/Pester.app" "${APP_INSTALL_DIR}/Pester.app"
  chmod 0755 "${APP_INSTALL_DIR}/Pester.app/Contents/MacOS/pester"
fi

"${INSTALL_DIR}/${BIN_NAME}" install

say "Pester installed to ${INSTALL_DIR}/${BIN_NAME}"
if [ "$IS_MACOS" -eq 1 ]; then
  say "Pester app bundle installed to ${HOME}/Applications/Pester.app"
fi
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    say ""
    say "${INSTALL_DIR} is not currently in PATH."
    say "Add this to your shell profile:"
    say "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

say ""
say "Try:"
say "  pester add winddown --time 22:00 --every 5m --title \"Wind down\" --message \"No exciting stuff now.\""
say "  pester status"
