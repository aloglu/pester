#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <target> <binary-path> <version>" >&2
  exit 1
fi

TARGET="$1"
BINARY="$2"
VERSION="$3"
DIST_DIR="dist"
APP_DIR="${DIST_DIR}/Pester.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"

rm -rf "$DIST_DIR"
mkdir -p "$MACOS_DIR"

cp "$BINARY" "${DIST_DIR}/pester"
cp "$BINARY" "${MACOS_DIR}/pester"
chmod 0755 "${DIST_DIR}/pester" "${MACOS_DIR}/pester"

cat > "${CONTENTS_DIR}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>pester</string>
  <key>CFBundleIdentifier</key>
  <string>com.aloglu.pester</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Pester</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSBackgroundOnly</key>
  <true/>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
</dict>
</plist>
PLIST

tar -C "$DIST_DIR" -czf "pester-${TARGET}.tar.gz" pester Pester.app
