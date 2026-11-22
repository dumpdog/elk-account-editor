#!/bin/bash
# Builds "Elk Account Editor.app" and a .dmg from the published macOS folder (dotnet publish ... -r osx-arm64 -p:PublishSingleFile=true).
# Run this on a Mac:   ./make-mac-app.sh path/to/Editor-osx-arm64
#
# Without any signing identity the app is signed "ad hoc": it runs, but the first start needs a right-click > Open (or System Settings > Privacy & Security > Open Anyway).
# For an app that opens with a plain double-click on any Mac, sign and notarize it (needs an Apple Developer ID, $99/year):
#   SIGN_ID="Developer ID Application: Your Name (TEAMID)" NOTARY_PROFILE=my-notary-profile ./make-mac-app.sh path/to/Editor-osx-arm64
# where my-notary-profile was stored once with:  xcrun notarytool store-credentials my-notary-profile --apple-id you@example.com --team-id TEAMID
set -euo pipefail
SRC="${1:?usage: make-mac-app.sh path/to/Editor-osx-arm64}"
NAME="Elk Account Editor"
APP="$NAME.app"
rm -rf "$APP" "$NAME.dmg"
mkdir -p "$APP/Contents/MacOS"
cp "$SRC/ElkAccountEditor" "$SRC/Photino.Native.dylib" "$APP/Contents/MacOS/"
chmod +x "$APP/Contents/MacOS/ElkAccountEditor"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Elk Account Editor</string>
  <key>CFBundleDisplayName</key><string>Elk Account Editor</string>
  <key>CFBundleIdentifier</key><string>local.elk.accounteditor</string>
  <key>CFBundleExecutable</key><string>ElkAccountEditor</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSLocalNetworkUsageDescription</key><string>Elk Account Editor connects to your Elk M1 alarm panel on the local network.</string>
</dict>
</plist>
PLIST
if [ -n "${SIGN_ID:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP/Contents/MacOS/Photino.Native.dylib"
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP/Contents/MacOS/ElkAccountEditor"
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP"
else
  codesign --force --sign - "$APP/Contents/MacOS/Photino.Native.dylib"
  codesign --force --sign - "$APP/Contents/MacOS/ElkAccountEditor"
  codesign --force --sign - "$APP"
fi
hdiutil create -volname "$NAME" -srcfolder "$APP" -ov -format UDZO "$NAME.dmg"
if [ -n "${SIGN_ID:-}" ] && [ -n "${NOTARY_PROFILE:-}" ]; then
  xcrun notarytool submit "$NAME.dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$NAME.dmg"
fi
echo "Done: $NAME.dmg (drag the app to Applications)"
