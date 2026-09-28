#!/bin/sh
# Wraps the spike in an ad-hoc signed .app so macOS treats it as its own app:
# its own permission prompts, and the App Sandbox when asked for.
#
#   ./bundle.sh sandboxed|unsandboxed <output dir>
#
# Run the result with:
#   open -W -n --stdout out.txt --stderr err.txt <app> --args record
set -eu
variant=$1
out=$2
cd "$(dirname "$0")"
cargo build --release
case $variant in
  sandboxed) id=com.sailvai.anchovy.audiospike.sandboxed name="Audio Spike Sandboxed" ;;
  unsandboxed) id=com.sailvai.anchovy.audiospike name="Audio Spike" ;;
  *) echo "variant must be sandboxed or unsandboxed" >&2; exit 1 ;;
esac
app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/audio-spike "$app/Contents/MacOS/"
sed -e "s/BUNDLE_ID/$id/" -e "s/BUNDLE_NAME/$name/" bundle/Info.plist > "$app/Contents/Info.plist"
codesign --force --sign - --options runtime --entitlements "bundle/$variant.entitlements" "$app"
echo "$app"
