#!/bin/bash
# Starts the editor in your browser instead of its own window (use this if the window does not open).
cd "$(dirname "$0")" || exit 1
APP="Elk Account Editor.app"
xattr -dr com.apple.quarantine "$APP" 2>/dev/null
codesign -v "$APP" >/dev/null 2>&1 || codesign -s - --force --deep "$APP" >/dev/null 2>&1
exec "$APP/Contents/MacOS/elkaccounteditor" --browser "$@"
