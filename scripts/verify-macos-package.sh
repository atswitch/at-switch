#!/usr/bin/env bash
# Read-only verification of the exact release DMG on a macOS host.
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Run this check on macOS.' >&2; exit 1; }
DMG=${1:?Usage: verify-macos-package.sh <dmg> <version> <developer-team-id>}
VERSION=${2:?Expected version is required}
TEAM=${3:?Expected Developer ID team is required}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ "$TEAM" =~ ^[A-Z0-9]{10}$ ]]
test -f "$DMG"
hdiutil verify "$DMG"
MOUNT=$(mktemp -d "${TMPDIR:-/tmp}/atswitch-release.XXXXXX")
mounted=0
cleanup() {
  result=$?
  trap - EXIT
  if [[ "$mounted" == 1 ]]; then
    hdiutil detach "$MOUNT" || { echo 'Failed to detach validation image.' >&2; exit 1; }
  fi
  rmdir "$MOUNT"
  exit "$result"
}
trap cleanup EXIT
hdiutil attach "$DMG" -readonly -nobrowse -mountpoint "$MOUNT"
mounted=1
APP="$MOUNT/AT-Switch.app"
PLIST="$APP/Contents/Info.plist"
test -f "$PLIST"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$PLIST")" = "$VERSION"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$PLIST")" = "$VERSION"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")" = com.atswitch.desktop
archs=$(lipo -archs "$APP/Contents/MacOS/at-switch")
[[ " $archs " == *' x86_64 '* && " $archs " == *' arm64 '* ]]
codesign --verify --deep --strict --verbose=2 "$APP"
details=$(codesign --display --verbose=4 "$APP" 2>&1)
printf '%s\n' "$details"
grep -Fq 'Authority=Developer ID Application:' <<< "$details"
grep -Fxq "TeamIdentifier=$TEAM" <<< "$details"
grep -Eq '^CodeDirectory .*flags=.*runtime' <<< "$details"
assessment=$(spctl --assess --type execute --verbose=4 "$APP" 2>&1)
printf '%s\n' "$assessment"
grep -Fq 'source=Notarized Developer ID' <<< "$assessment"
# Tauri normally staples the app before creating the disk image. Also accept
# a validated ticket on the distributed DMG; do not rewrite either artifact.
if xcrun stapler validate "$APP"; then
  echo 'Stapled notarization ticket verified on the app.'
else
  xcrun stapler validate "$DMG"
  echo 'Stapled notarization ticket verified on the DMG.'
fi
shasum -a 256 "$DMG"
echo "MACOS_PACKAGE_VERIFIED=$VERSION"
