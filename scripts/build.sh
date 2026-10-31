#!/bin/sh
# builds dist/Asahiro WuWa.app from the rust core and swift front end
set -e
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=26.0
# ci passes the release version and run number, local builds keep the defaults
VERSION="${VERSION:-0.1.0}"
BUILD="${BUILD:-1}"

# the injector runs inside wine, the core embeds the committed exe so mingw is only needed to change it
if command -v x86_64-w64-mingw32-gcc >/dev/null; then
    x86_64-w64-mingw32-gcc -O2 -municode -static -s -Wall -o app/Resources/bin/asahiro-inject.exe app/inject/asahiro-inject.c
    # starts steam's web helper with the flags that keep its windows from rendering black
    x86_64-w64-mingw32-gcc -O2 -municode -mwindows -static -s -Wall -Wextra -o app/Resources/bin/steamwebhelper.exe app/inject/steamwebhelper-wrapper.c
fi
# the frame pacer and dock icon hooks load into the x86_64 wine, the core embeds the committed dylib like the injector
xcrun clang -arch x86_64 -O2 -Wall -fno-objc-arc -dynamiclib -lobjc -o app/Resources/bin/libasahiro-hooks.dylib app/hooks/asahiro-hooks.m
cargo build --release --manifest-path core/Cargo.toml
rm -f app/.build/release/Asahiro
swift build -c release --package-path app

# the fork's identity, see app/Sources/Asahiro/AppInfo.swift. the executable name differs from upstream's
# because the game shortcut finds the running launcher by process name
NAME="Asahiro WuWa"
BUNDLE_ID=com.yasingunaydin.asahiro-wuwa
EXE=AsahiroWuWa
APP="dist/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$(swift build -c release --package-path app --show-bin-path)/Asahiro" "$APP/Contents/MacOS/$EXE"
# compiles the icon composer document into the liquid glass asset catalog plus a legacy icns
# actool ships with xcode only, a command line tools setup still gets a working app, just without the icon
if xcrun -f actool >/dev/null 2>&1; then
    xcrun actool app/Resources/AppIcon.icon --compile "$APP/Contents/Resources" --platform macosx \
        --minimum-deployment-target 26.0 --app-icon AppIcon --target-device mac \
        --output-partial-info-plist /dev/null >/dev/null
else
    echo "actool not found (it comes with xcode), building without the app icon"
fi
mkdir -p "$APP/Contents/Resources/Fonts"
cp app/Resources/Fonts/*.ttf app/Resources/Fonts/OFL.txt "$APP/Contents/Resources/Fonts/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key><string>$EXE</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleIconName</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD</string>
    <key>LSMinimumSystemVersion</key><string>26.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>ATSApplicationFontsPath</key><string>Fonts</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>$BUNDLE_ID</string>
            <key>CFBundleURLSchemes</key><array><string>asahiro-wuwa</string></array>
        </dict>
    </array>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "built $APP"
