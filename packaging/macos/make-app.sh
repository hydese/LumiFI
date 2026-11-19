#!/usr/bin/env bash
# Builds LumiFi.app around a compiled binary.
# Usage: packaging/macos/make-app.sh <path to lumifi binary> <version> <output dir>
set -euo pipefail
binary="$1"; version="$2"; out="$3"
app="$out/LumiFi.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/lumifi"
sed "s/@VERSION@/$version/g" "$(dirname "$0")/Info.plist" > "$app/Contents/Info.plist"
# Ad-hoc signature: Apple Silicon needs one to run. It is not an Apple Developer ID, so the first
# launch still needs right-click > Open (or: xattr -dr com.apple.quarantine LumiFi.app).
codesign --force --deep --sign - "$app"
echo "$app"
