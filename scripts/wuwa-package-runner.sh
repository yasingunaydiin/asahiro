#!/bin/sh
# packages the patched runner that the app downloads: yaagl's crossover 11.0 runner, the same archive
# upstream asahiro uses, with the modules from wine/patches swapped in by wuwa-build-runner.sh. asahiro
# itself adds dxmt and the game bundle after unpacking, like it does for the stock runner.
#
#   ./scripts/wuwa-package-runner.sh    writes $WUWA_ROOT/package/$NAME.tar.xz and prints its sha256 and size
#
# the result goes into a pre-release on github, see RUNNER_URL in core/src/runner.rs
set -e

ROOT="${WUWA_ROOT:-/Volumes/T7/WuWa-test}"
NAME="wine-crossover-11.0-1-wuwa1"
BASE_URL="https://github.com/yaagl/anime-game-wine/releases/download/wine-crossover-11.0-1-signed/wine-crossover-11.0-1-osx64-signed.tar.xz"
BASE_SHA256="89fa7e90fb626523a90d5867a03c6be785d017176739c6320a3b86c7838c3a35"
PKG="$ROOT/package"
BASE="$PKG/base.tar.xz"

if [ ! -d "$(dirname "$ROOT")" ]; then
    echo "$(dirname "$ROOT") is not mounted, plug in the ssd or set WUWA_ROOT" >&2
    exit 1
fi
mkdir -p "$PKG"
if ! echo "$BASE_SHA256  $BASE" | shasum -a 256 -c >/dev/null 2>&1; then
    echo "downloading yaagl's runner (about 450 MB)"
    curl -fL --progress-bar -o "$BASE" "$BASE_URL"
    echo "$BASE_SHA256  $BASE" | shasum -a 256 -c >/dev/null || { echo "runner checksum mismatch" >&2; exit 1; }
fi

rm -rf "$PKG/base" "$PKG/$NAME"
mkdir -p "$PKG/base"
tar -xJf "$BASE" -C "$PKG/base"
wine=$(find "$PKG/base" -maxdepth 5 -path '*/bin/wine' -type f | head -1)
[ -n "$wine" ] || { echo "the runner archive has an unexpected layout" >&2; exit 1; }
WUWA_BASE_RUNNER="$(dirname "$(dirname "$wine")")" "$(dirname "$0")/wuwa-build-runner.sh" "$PKG/$NAME"
rm -f "$PKG/$NAME/.wuwa-patches"

echo "compressing"
rm -f "$PKG/$NAME.tar.xz"
tar -cJf "$PKG/$NAME.tar.xz" -C "$PKG" "$NAME"
rm -rf "$PKG/base"
echo "$PKG/$NAME.tar.xz"
echo "sha256 $(shasum -a 256 "$PKG/$NAME.tar.xz" | cut -d' ' -f1)"
echo "size   $(stat -f %z "$PKG/$NAME.tar.xz")"
