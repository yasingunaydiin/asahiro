#!/bin/sh
# builds dist/Asahiro Waves.app from the rust core and swift front end
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
NAME="Asahiro Waves"
BUNDLE_ID=com.yasingunaydin.asahiro-waves
EXE=AsahiroWaves
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
    # without xcode, a flat icns from the same plate and glyph on the dark rounded square. the icon
    # composer canvas is the icon body, which takes the inner 824 of macos's 1024 icon grid
    echo "actool not found (it comes with xcode), building a flat app icon"
    ICON_TMP=$(mktemp -d)
    {
        echo '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        echo '<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="0.8"><stop offset="0" stop-color="#2b2b30"/><stop offset="1" stop-color="#0d0d0f"/></linearGradient></defs>'
        echo '<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bg)"/>'
        echo '<g transform="translate(100 100) scale(0.8047)">'
        grep -h '<path' app/Resources/AppIcon.icon/Assets/plate.svg app/Resources/AppIcon.icon/Assets/glyph.svg
        echo '</g></svg>'
    } > "$ICON_TMP/AppIcon.svg"
    # appkit keeps the corners transparent, quick look's renderer fills them white
    cat > "$ICON_TMP/render.swift" <<'SWIFT'
import AppKit
let a = CommandLine.arguments
let img = NSImage(contentsOf: URL(fileURLWithPath: a[1]))!
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 1024, bitsPerSample: 8, samplesPerPixel: 4,
                           hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
img.draw(in: NSRect(x: 0, y: 0, width: 1024, height: 1024))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[2]))
SWIFT
    swift "$ICON_TMP/render.swift" "$ICON_TMP/AppIcon.svg" "$ICON_TMP/AppIcon.svg.png"
    mkdir -p "$ICON_TMP/AppIcon.iconset"
    for s in 16 32 128 256 512; do
        sips -z $s $s "$ICON_TMP/AppIcon.svg.png" --out "$ICON_TMP/AppIcon.iconset/icon_${s}x${s}.png" >/dev/null
        sips -z $((s * 2)) $((s * 2)) "$ICON_TMP/AppIcon.svg.png" --out "$ICON_TMP/AppIcon.iconset/icon_${s}x${s}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICON_TMP/AppIcon.iconset" -o "$APP/Contents/Resources/AppIcon.icns" \
        || echo "could not build the app icon, building without it"
    rm -rf "$ICON_TMP"
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
            <key>CFBundleURLSchemes</key><array><string>asahiro-waves</string></array>
        </dict>
    </array>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "built $APP"
