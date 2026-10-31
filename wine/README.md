# Wine patches for Wuthering Waves

These patches let the Steam release of Wuthering Waves, including its ACE kernel anti-cheat, run
under Wine on Apple Silicon. They go on top of CodeWeavers' CrossOver 26.1.0 Wine source, which is
the Wine 11.0 that asahiro's runner is built from.

None of them disable or work around the anti-cheat. They make Wine behave more like Windows, the
same way dwproton and GE-Proton do on Linux, where Kuro supports the game on the Steam Deck.

## Base

- `crossover-sources-26.1.0.tar.gz` from <https://media.codeweavers.com/pub/crossover/source/>
- sha256 `e4ec87d5821a009dd1f1d2e36ffe2e24b8fcbae9516375ea42f95a16928ab8fa`
- only `sources/wine` is used

## What the patches do

| Patches | What | From |
|---|---|---|
| 0001, 0029 | Rosetta 2 raises SIGILL for register form hint nops (`0f 18`–`0f 1f`, mod 3) that x86 cpus treat as nops, and #UD instead of #GP for privileged instructions. The game's protector is full of the former, ACE's driver reads `cr3`. | new |
| 0002–0026, 0028, 0031 | ntoskrnl functions ACE's driver uses: guarded mutexes, `PsGetProcess*`, `PsGetThread*`, bug check callbacks, `MmGetVirtualForPhysical`, `PsGetContextThread`, tokens as kernel objects | dwproton 11.0-12 (Etaash Mathamsetty, Quitting) |
| 0027 | `PdhCloseQuery` on a bad handle, hit by Tencent PerfSight | dwproton, upstream MR 11739 (Ziia Shi) |
| 0030, 0040, 0041, 0042 | `PsGetProcessExitStatus`, `SeQueryInformationToken`, HAL clock and stall, no-op `Inbv*` | new |
| 0032–0034, 0036–0039 | DriverEntry called with a kernel-space return address, `MmCopyMemory`, `ServiceKeyName`, MDL section views, `PsGetContextThread` thunk, lsass.exe SamSs service | GE-Proton `patches/game-patches/ace-*` (mkrsym1, NelloKudo, Piotr Caban) |
| 0035 | upstream Wine's MDL map and lock functions, which CrossOver 26.1 still stubs | upstream Wine |

Patches taken from elsewhere keep their author. Where one needed changes to apply to CrossOver 26.1,
its message says what changed.

## Building

The runner is x86_64 and runs under Rosetta 2, so configure and make run under `arch -x86_64`. Wine
needs bison 3 (`brew install bison`) and mingw-w64 (`brew install mingw-w64`). Patch 0038 adds a
program, so `configure` has to be regenerated with autoconf.

```sh
tar xzf crossover-sources-26.1.0.tar.gz sources/wine
cd sources/wine
git init -q && git add -A && git commit -qm base
git am /path/to/asahiro/wine/patches/*.patch
autoconf
mkdir ../build64 && cd ../build64
PATH="$(brew --prefix bison)/bin:$PATH" MACOSX_DEPLOYMENT_TARGET=14.0 arch -x86_64 ../wine/configure \
    CC="clang -arch x86_64" --enable-win64 --enable-archs=i386,x86_64 --with-mingw \
    --without-x --without-freetype --without-gnutls --without-gstreamer --without-vulkan \
    --without-opencl --without-sdl --without-cups --without-pcap --without-inotify \
    --disable-tests --disable-winemenubuilder
```

Only the modules the patches touch are built and swapped into asahiro's runner. Everything else
stays as shipped.

```sh
PATH="$(brew --prefix bison)/bin:$PATH" MACOSX_DEPLOYMENT_TARGET=14.0 arch -x86_64 make -j8 \
    dlls/ntdll/ntdll.so server/wineserver \
    dlls/ntoskrnl.exe/x86_64-windows/ntoskrnl.exe dlls/hal/x86_64-windows/hal.dll \
    dlls/pdh/x86_64-windows/pdh.dll programs/lsass/x86_64-windows/lsass.exe \
    programs/wineboot/x86_64-windows/wineboot.exe
```

| Built file | Goes to, in the runner |
|---|---|
| `dlls/ntdll/ntdll.so` | `lib/wine/x86_64-unix/` (ad-hoc sign it) |
| `server/wineserver` | `bin/` (ad-hoc sign it) |
| the `.exe` and `.dll` files | `lib/wine/x86_64-windows/` |

`ntdll.so` and `wineserver` have to be swapped together. The server protocol is unchanged, so the
runner's `win32u.so` keeps working with them.

## Bottle setup

The bottle needs `lsass.exe` in `system32` and the SamSs service, which ACE-CORE wants running. On a
bottle made by the runner's own `wine.inf`:

```sh
cp programs/lsass/x86_64-windows/lsass.exe "$WINEPREFIX/drive_c/windows/system32/"
wine sc create SamSs binPath= "C:\windows\system32\lsass.exe" type= share start= demand \
    DisplayName= "Security Accounts Manager"
```

The patched wineboot starts it on every boot.

## Known gaps

- dwproton 11.0-16 also reports loaded kernel drivers in `SystemModuleInformation`, adds system
  threads and a csrss stub, and runs lsass in session 0. These change the server protocol and aren't
  ported yet. The game runs without them.
- Many ntoskrnl, fltmgr and ndis functions that ACE imports are still stubs. The ones that crashed
  the driver are implemented. The rest haven't been called in testing so far.
