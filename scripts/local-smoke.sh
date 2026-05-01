#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_ROOT="/tmp/pester-test-config"
STATE_ROOT="/tmp/pester-test-state"

cd "$ROOT_DIR"

mkdir -p "$CONFIG_ROOT" "$STATE_ROOT"

export XDG_CONFIG_HOME="$CONFIG_ROOT"
export XDG_STATE_HOME="$STATE_ROOT"

cat <<EOF
Local pester smoke environment

Config root: $XDG_CONFIG_HOME
State root:  $XDG_STATE_HOME

Building debug binaries...
EOF

cargo build --bin pester --bin pesterd

cat <<EOF

Build complete.

Start the daemon in one terminal:
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pesterd

In another terminal, run commands like:
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pester timer tea 10s --title "Tea is ready"
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pester timer list
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pester timer stop tea

For reminder notifications:
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pester add stretch --time 23:00 --every 5m --title "Stretch" --message "Stand up"
  XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME $ROOT_DIR/target/debug/pester test stretch

When done, clean up with:
  rm -rf $XDG_CONFIG_HOME $XDG_STATE_HOME
EOF
