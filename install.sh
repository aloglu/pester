#!/bin/sh
set -eu

REPO="aloglu/pester"
BIN_NAME="pester"
STEP=0
TOTAL_STEPS=5

say() {
  printf '%s\n' "$1"
}

supports_color() {
  [ -t 1 ] || return 1
  [ "${NO_COLOR:-}" = "" ] || return 1
  [ "${PESTER_INSTALL_NO_COLOR:-}" = "" ] || return 1
  [ "${TERM:-}" != "dumb" ] || return 1
  return 0
}

if supports_color; then
  BOLD="$(printf '\033[1m')"
  DIM="$(printf '\033[2m')"
  GREEN="$(printf '\033[32m')"
  YELLOW="$(printf '\033[33m')"
  BLUE="$(printf '\033[34m')"
  RED="$(printf '\033[31m')"
  RESET="$(printf '\033[0m')"
else
  BOLD=""
  DIM=""
  GREEN=""
  YELLOW=""
  BLUE=""
  RED=""
  RESET=""
fi

heading() {
  say "${BOLD}Pester Installer${RESET}"
}

detail() {
  say "  ${DIM}$1${RESET}"
}

details() {
  printf '%s\n' "$1" | while IFS= read -r line; do
    detail "$line"
  done
}

step() {
  STEP=$((STEP + 1))
  say ""
  say "${BLUE}[${STEP}/${TOTAL_STEPS}]${RESET} ${BOLD}$1${RESET}"
}

ok() {
  say "  ${GREEN}OK${RESET} $1"
}

warn() {
  say "  ${YELLOW}WARN${RESET} $1"
}

fail() {
  say "  ${RED}ERROR${RESET} $1"
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "Required command not found: $1"
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
    fail "Unsupported OS: $OS"
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64 | amd64)
    if [ "$IS_MACOS" -eq 1 ]; then
      fail "Intel macOS is not supported yet. Use an Apple Silicon Mac or a different platform."
      exit 1
    fi
    TARGET_ARCH="x86_64"
    ;;
  arm64 | aarch64) TARGET_ARCH="aarch64" ;;
  *)
    fail "Unsupported architecture: $ARCH"
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

heading
detail "Target: ${OS} ${TARGET_ARCH}"
detail "Artifact: ${ARTIFACT}"

step "Downloading release files"
curl -fsSL "${BASE_URL}/${ARTIFACT}" -o "${TMP_DIR}/${ARTIFACT}"
curl -fsSL "${BASE_URL}/checksums.txt" -o "${TMP_DIR}/checksums.txt"
ok "Downloaded ${ARTIFACT}"

step "Verifying checksum"
if ! checksum_entry "$ARTIFACT" "${TMP_DIR}/checksums.txt" > "${TMP_DIR}/checksum.txt"; then
  fail "Checksum entry not found for ${ARTIFACT}."
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && sha256sum -c checksum.txt >/dev/null)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && shasum -a 256 -c checksum.txt >/dev/null)
else
  fail "Could not verify checksum: sha256sum or shasum is required."
  exit 1
fi
ok "Checksum verified"

step "Installing binary"
mkdir -p "$INSTALL_DIR"
tar -xzf "${TMP_DIR}/${ARTIFACT}" -C "$TMP_DIR"
install -m 0755 "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
ok "Installed to ${INSTALL_DIR}/${BIN_NAME}"

if [ "$IS_MACOS" -eq 1 ]; then
  APP_INSTALL_DIR="${HOME}/Applications"
  mkdir -p "$APP_INSTALL_DIR"
  rm -rf "${APP_INSTALL_DIR}/Pester.app"
  cp -R "${TMP_DIR}/Pester.app" "${APP_INSTALL_DIR}/Pester.app"
  chmod 0755 "${APP_INSTALL_DIR}/Pester.app/Contents/MacOS/pester"
  ok "Installed app bundle to ${HOME}/Applications/Pester.app"
fi

step "Starting background service"
if ! INSTALL_OUTPUT="$("${INSTALL_DIR}/${BIN_NAME}" install 2>&1)"; then
  fail "Background service installation failed."
  if [ "$INSTALL_OUTPUT" != "" ]; then
    details "$INSTALL_OUTPUT"
  fi
  exit 1
fi
if [ "$INSTALL_OUTPUT" != "" ]; then
  details "$INSTALL_OUTPUT"
fi
ok "Background service installed and started"

step "Finishing setup"
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ok "Pester is ready" ;;
  *)
    warn "${INSTALL_DIR} is not currently in PATH."
    detail "Add this to your shell profile:"
    detail "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

say ""
say "${BOLD}Next steps:${RESET}"
detail "pester add winddown --time 22:00 --every 5m --title \"Wind down\" --message \"No exciting stuff now.\""
detail "pester status"
