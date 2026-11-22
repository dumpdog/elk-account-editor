#!/bin/bash
# Builds "Elk Account Editor.app" from the release binary, signs it ad hoc and zips it with the README.
# Run on a Mac:
#   rust/packaging/make-mac-app.sh <binary> <zip name without .zip> [outdir]
#   e.g. make-mac-app.sh rust/app/target/aarch64-apple-darwin/release/elkaccounteditor ElkAccountEditor-mac-arm64 dist
# Without SIGN_ID the app gets an ad-hoc signature (codesign -s -): it runs, but a downloaded copy needs the xattr step in
# README-mac.txt. For a double-click app on any Mac set SIGN_ID="Developer ID Application: Name (TEAMID)" and
# NOTARY_PROFILE (see `xcrun notarytool store-credentials`); the app is then notarized and stapled.
# APP_VERSION (for example 0.2.0) is written into Info.plist; CI takes it from rust/app/Cargo.toml.
set -euo pipefail
BIN="${1:?usage: make-mac-app.sh <binary> <zip name> [outdir]}"
ZIPNAME="${2:?usage: make-mac-app.sh <binary> <zip name> [outdir]}"
OUT="${3:-dist}"
HERE="$(cd "$(dirname "$0")" && pwd)"
NAME="Elk Account Editor"
STAGE="${OUT:?}/${ZIPNAME:?}"
ZIP="${OUT:?}/${ZIPNAME:?}.zip"
APP="$STAGE/$NAME.app"
rm -rf "${STAGE:?}" "${ZIP:?}"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/elkaccounteditor"
chmod +x "$APP/Contents/MacOS/elkaccounteditor"
if [ -f "$HERE/AppIcon.icns" ]; then
  cp "$HERE/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
  ICON='<key>CFBundleIconFile</key><string>AppIcon</string>'
else
  ICON=''
fi
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>local.elk.accounteditor</string>
  <key>CFBundleExecutable</key><string>elkaccounteditor</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${APP_VERSION:-1.0}</string>
  <key>CFBundleVersion</key><string>${APP_VERSION:-1}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
  <key>NSLocalNetworkUsageDescription</key><string>Elk Account Editor connects to your Elk M1 alarm panel on the local network.</string>
  $ICON
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist"
if [ -n "${SIGN_ID:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP/Contents/MacOS/elkaccounteditor"
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP"
else
  codesign -s - --force --deep "$APP"
fi
codesign --verify --deep --strict "$APP"
cp "$HERE/README-mac.txt" "$STAGE/README.txt"
cp "$HERE/Start in browser.command" "$STAGE/Start in browser.command"
chmod +x "$STAGE/Start in browser.command"
if [ -n "${SIGN_ID:-}" ] && [ -n "${NOTARY_PROFILE:-}" ]; then
  ditto -c -k --keepParent "$APP" "$OUT/notarize-$ZIPNAME.zip"
  xcrun notarytool submit "$OUT/notarize-$ZIPNAME.zip" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP"
  rm -f "${OUT:?}/notarize-$ZIPNAME.zip"
fi
# ditto keeps the executable bits and the bundle intact
ditto -c -k --keepParent "$STAGE" "$ZIP"
echo "Done: $ZIP"
