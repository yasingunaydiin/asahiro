#!/bin/sh
# first test for wuthering waves: does the windows steam build start under asahiro's wine on macos?
# on linux it runs under proton with SteamOS=1, which makes the game take its steam deck path for the
# anti-cheat. nothing here patches or disables the anti-cheat, if it refuses to start the test is over.
#
# everything lives in $WUWA_ROOT (default /Volumes/T7/WuWa-test), asahiro's own runner and bottle are
# only read from, never changed.
#
#   ./scripts/wuwa-test.sh setup    copy the wine runner and create a fresh bottle
#   ./scripts/wuwa-test.sh steam    download and run the windows steam installer
#   ./scripts/wuwa-test.sh run      start steam with SteamOS=1, install and launch wuwa from there
#   ./scripts/wuwa-test.sh status   show wine processes and the anti-cheat files in the game folder
#   ./scripts/wuwa-test.sh stop     kill everything running in the test bottle
#   ./scripts/wuwa-test.sh logs     show the end of the latest log
#   ./scripts/wuwa-test.sh relink   point the runner copy's symlinks at itself (setup does this)
set -e

ROOT="${WUWA_ROOT:-/Volumes/T7/WuWa-test}"
SRC_RUNNER="$HOME/Library/Application Support/Asahiro/runners/wine-crossover-11.0-1"
RUNNER="$ROOT/runner"
export WINEPREFIX="$ROOT/bottle"
LOGS="$ROOT/logs"
STEAM_URL="https://cdn.cloudflare.steamstatic.com/client/installer/SteamSetup.exe"
STEAM_EXE="C:\\Program Files (x86)\\Steam\\steam.exe"
GAME_DIR="$WINEPREFIX/drive_c/Program Files (x86)/Steam/steamapps/common/Wuthering Waves"

# same environment asahiro gives its bundled runner, see Runner::wine in core/src/wine.rs
export WINEMSYNC=1
export WINEDEBUG="${WINEDEBUG:--all}"
export GST_PLUGIN_FEATURE_RANK="atdec:MAX,avdec_h264:MAX"
export DXMT_SHADER_CACHE_PATH="$ROOT/shadercache"

# steam's web views tend to render black under wine, these keep them on the cpu. override with STEAM_ARGS
STEAM_ARGS="${STEAM_ARGS:--cef-disable-gpu -cef-disable-sandbox}"

wine() { "$RUNNER/bin/wine" "$@"; }
wineserver() { "$RUNNER/bin/wineserver" "$@"; }
log_file() { mkdir -p "$LOGS"; echo "$LOGS/$1-$(date +%Y%m%d-%H%M%S).log"; }

need_root() {
    if [ ! -d "$(dirname "$ROOT")" ]; then
        echo "$(dirname "$ROOT") is not mounted, plug in the ssd or set WUWA_ROOT" >&2
        exit 1
    fi
}

# asahiro's runner has absolute symlinks into itself (its zzz app bundle for the dock name), point them
# at the copy so the test never runs asahiro's own binaries
relink_runner() {
    find "$RUNNER" -type l | while IFS= read -r l; do
        t=$(readlink "$l")
        case "$t" in "$SRC_RUNNER"/*) ln -sfh "$RUNNER${t#"$SRC_RUNNER"}" "$l" ;; esac
    done
}

need_bottle() {
    need_root
    if [ ! -x "$RUNNER/bin/wine" ] || [ ! -d "$WINEPREFIX/drive_c" ]; then
        echo "no test bottle yet, run: $0 setup" >&2
        exit 1
    fi
}

case "$1" in
relink)
    need_bottle
    relink_runner
    echo "runner links now point inside $RUNNER"
    ;;
setup)
    need_root
    if [ ! -x "$SRC_RUNNER/bin/wine" ]; then
        echo "asahiro's runner is missing at $SRC_RUNNER, launch zzz in asahiro once first" >&2
        exit 1
    fi
    mkdir -p "$ROOT" "$LOGS" "$DXMT_SHADER_CACHE_PATH"
    # a private copy, so an asahiro update or graphics switch can't change the runner mid-test.
    # it already has dxmt swapped in, which is what asahiro uses for zzz
    echo "copying the wine runner (about 2 GB)"
    rsync -a --delete "$SRC_RUNNER/" "$RUNNER/"
    relink_runner
    echo "creating the bottle at $WINEPREFIX"
    L=$(log_file setup)
    wine wineboot -u >"$L" 2>&1
    wineserver -w
    echo "done, next: $0 steam"
    ;;
steam)
    need_bottle
    mkdir -p "$ROOT/downloads"
    curl -fL --progress-bar -o "$ROOT/downloads/SteamSetup.exe" "$STEAM_URL"
    L=$(log_file steam-install)
    echo "the steam installer window should open now, click through it and untick 'run steam' at the end"
    wine "$ROOT/downloads/SteamSetup.exe" >"$L" 2>&1 || true
    wineserver -w
    echo "done, next: $0 run"
    ;;
run)
    need_bottle
    # inherited by the game when steam starts it, the windows steam client has no %command% launch options
    export SteamOS=1
    L=$(log_file steam)
    echo "starting steam, log: $L"
    echo "sign in, add wuthering waves to your library and install it, then press play"
    # shellcheck disable=SC2086
    wine "$STEAM_EXE" $STEAM_ARGS >"$L" 2>&1 &
    ;;
status)
    need_root
    ps -axo pid,etime,args | grep -iE 'wine|steam|wuthering|Client-Win64|ACE-[A-Za-z]+\.' | grep -v grep || echo "nothing running"
    if [ -d "$GAME_DIR" ]; then
        echo "game folder: $(du -sh "$GAME_DIR" | cut -f1)"
        find "$GAME_DIR" -maxdepth 4 -iname '*ace*' \( -iname '*.sys' -o -iname '*.exe' -o -iname '*.dll' \) | sed "s|$GAME_DIR/||"
    else
        echo "game not installed yet"
    fi
    ;;
stop)
    need_bottle
    wineserver -k || true
    echo "stopped"
    ;;
logs)
    need_root
    latest=$(ls -t "$LOGS"/*.log 2>/dev/null | head -1)
    [ -n "$latest" ] && { echo "== $latest"; tail -n "${2:-60}" "$latest"; } || echo "no logs yet"
    ;;
*)
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
