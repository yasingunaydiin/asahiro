#!/bin/sh
# builds the patched wine runner for wuthering waves: crossover's wine source with wine/patches applied,
# then only the modules the patches touch are built and swapped into a copy of asahiro's runner.
# see wine/README.md for what the patches do.
#
#   ./scripts/wuwa-build-runner.sh [out]    out defaults to $WUWA_ROOT/runner
#
# the source, build tree and tarball are kept in $WUWA_ROOT/build so a second run only rebuilds what
# changed. asahiro's own runner is only read from.
set -e

ROOT="${WUWA_ROOT:-/Volumes/T7/WuWa-test}"
OUT="${1:-$ROOT/runner}"
WORK="$ROOT/build"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PATCHES="$REPO/wine/patches"
SRC_RUNNER="$HOME/Library/Application Support/Asahiro/runners/wine-crossover-11.0-1"
TARBALL_URL="https://media.codeweavers.com/pub/crossover/source/crossover-sources-26.1.0.tar.gz"
TARBALL_SHA256="e4ec87d5821a009dd1f1d2e36ffe2e24b8fcbae9516375ea42f95a16928ab8fa"
TARBALL="$WORK/crossover-sources-26.1.0.tar.gz"
SRC="$WORK/wine"
BUILD="$WORK/build64"

# built file -> where it goes in the runner
TARGETS="
dlls/ntdll/ntdll.so lib/wine/x86_64-unix/ntdll.so
server/wineserver bin/wineserver
dlls/ntoskrnl.exe/x86_64-windows/ntoskrnl.exe lib/wine/x86_64-windows/ntoskrnl.exe
dlls/hal/x86_64-windows/hal.dll lib/wine/x86_64-windows/hal.dll
dlls/pdh/x86_64-windows/pdh.dll lib/wine/x86_64-windows/pdh.dll
programs/lsass/x86_64-windows/lsass.exe lib/wine/x86_64-windows/lsass.exe
programs/wineboot/x86_64-windows/wineboot.exe lib/wine/x86_64-windows/wineboot.exe
"

if [ ! -d "$(dirname "$ROOT")" ]; then
    echo "$(dirname "$ROOT") is not mounted, plug in the ssd or set WUWA_ROOT" >&2
    exit 1
fi
if [ ! -x "$SRC_RUNNER/bin/wine" ]; then
    echo "asahiro's runner is missing at $SRC_RUNNER, launch zzz in asahiro once first" >&2
    exit 1
fi
BISON="$(brew --prefix bison 2>/dev/null)/bin"
for tool in "$BISON/bison" x86_64-w64-mingw32-gcc i686-w64-mingw32-gcc autoconf git; do
    command -v "$tool" >/dev/null || { echo "missing $tool, see wine/README.md (brew install bison mingw-w64 autoconf)" >&2; exit 1; }
done
# wine's tools and configure checks run as x86_64 under rosetta, like the runner
x86() { PATH="$BISON:$PATH" MACOSX_DEPLOYMENT_TARGET=14.0 arch -x86_64 "$@"; }

mkdir -p "$WORK"
if ! echo "$TARBALL_SHA256  $TARBALL" | shasum -a 256 -c >/dev/null 2>&1; then
    echo "downloading crossover's wine source (about 150 MB)"
    curl -fL --progress-bar -o "$TARBALL" "$TARBALL_URL"
    echo "$TARBALL_SHA256  $TARBALL" | shasum -a 256 -c >/dev/null || { echo "source checksum mismatch" >&2; exit 1; }
fi

# a fresh tree whenever the patches change, the stamp is the hash of the series
stamp=$(cat "$PATCHES"/*.patch | shasum -a 256 | cut -c1-16)
if [ "$(cat "$SRC/.wuwa-patches" 2>/dev/null)" != "$stamp" ]; then
    echo "applying $(ls "$PATCHES"/*.patch | wc -l | tr -d ' ') patches"
    rm -rf "$SRC" "$BUILD" "$WORK/sources"
    tar xzf "$TARBALL" -C "$WORK" sources/wine
    mv "$WORK/sources/wine" "$SRC" && rmdir "$WORK/sources"
    (
        cd "$SRC"
        g() { git -c user.name=wuwa-build -c user.email=wuwa-build@localhost "$@"; }
        g init -q && g add -A && g commit -qm "crossover-sources-26.1.0"
        g am -q "$PATCHES"/*.patch
        autoconf
    )
    echo "$stamp" > "$SRC/.wuwa-patches"
fi

if [ ! -f "$BUILD/Makefile" ]; then
    echo "configuring"
    mkdir -p "$BUILD"
    (cd "$BUILD" && x86 "$SRC/configure" CC="clang -arch x86_64" --enable-win64 --enable-archs=i386,x86_64 \
        --with-mingw --without-x --without-freetype --without-gnutls --without-gstreamer --without-vulkan \
        --without-opencl --without-sdl --without-cups --without-pcap --without-inotify \
        --disable-tests --disable-winemenubuilder > configure.log 2>&1) \
        || { echo "configure failed, see $BUILD/configure.log" >&2; exit 1; }
fi

echo "building"
# shellcheck disable=SC2086
(cd "$BUILD" && x86 make -j"$(sysctl -n hw.ncpu)" $(echo "$TARGETS" | awk 'NF {print $1}') > make.log 2>&1) \
    || { grep -E 'error' "$BUILD/make.log" | head -20; echo "build failed, see $BUILD/make.log" >&2; exit 1; }

echo "assembling the runner at $OUT"
rsync -a --delete "$SRC_RUNNER/" "$OUT/"
# asahiro's runner has absolute symlinks into itself (its zzz app bundle for the dock name)
find "$OUT" -type l | while IFS= read -r l; do
    t=$(readlink "$l")
    case "$t" in "$SRC_RUNNER"/*) ln -sfh "$OUT${t#"$SRC_RUNNER"}" "$l" ;; esac
done
echo "$TARGETS" | while read -r built dest; do
    [ -n "$built" ] || continue
    rm -f "$OUT/$dest"
    cp "$BUILD/$built" "$OUT/$dest"
    case "$dest" in *.so|bin/*) codesign -s - -f "$OUT/$dest" 2>/dev/null ;; esac
done
echo "$stamp" > "$OUT/.wuwa-patches"
echo "done"
