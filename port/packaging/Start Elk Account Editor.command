#!/bin/bash
# Prepares the Elk Account Editor for this Mac (once) and starts it. Apple Silicon only.
cd "$(dirname "$0")" || exit 1
if [ ! -f .prepared ] || ! codesign -v ElkAccountEditor >/dev/null 2>&1; then
  echo "Preparing the editor for this Mac (first run)..."
  chmod +x ElkAccountEditor
  xattr -dr com.apple.quarantine . 2>/dev/null
  codesign -s - --force Photino.Native.dylib >/dev/null 2>&1
  codesign -s - --force ElkAccountEditor >/dev/null 2>&1
  touch .prepared
fi
exec ./ElkAccountEditor
