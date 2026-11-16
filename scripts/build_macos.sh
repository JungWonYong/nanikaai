#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="NanikaAi"
APP_BUNDLE="$ROOT_DIR/dist/$APP_NAME.app"
BIN_PATH="$ROOT_DIR/target/release/nanikaai"
ICON_PNG="$ROOT_DIR/assets/icon.png"

echo "[1/4] Building release binary (100% Safe Rust)..."
cargo build --release

echo "[2/4] Generating AppIcon.icns..."
ICONSET_DIR="$ROOT_DIR/target/AppIcon.iconset"
ICNS_PATH="$ROOT_DIR/target/AppIcon.icns"
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

if [ -f "$ICON_PNG" ]; then
    for sz in 16 32 64 128 256 512; do
        sips -z "$sz" "$sz" "$ICON_PNG" --out "$ICONSET_DIR/icon_${sz}x${sz}.png" >/dev/null
        dbl=$((sz * 2))
        if [ "$dbl" -le 512 ]; then
            sips -z "$dbl" "$dbl" "$ICON_PNG" --out "$ICONSET_DIR/icon_${sz}x${sz}@2x.png" >/dev/null
        fi
    done
    iconutil -c icns "$ICONSET_DIR" -o "$ICNS_PATH"
    rm -rf "$ICONSET_DIR"
fi

echo "[3/4] Assembling $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$BIN_PATH" "$APP_BUNDLE/Contents/MacOS/nanikaai"
chmod +x "$APP_BUNDLE/Contents/MacOS/nanikaai"
cp -R "$ROOT_DIR/assets" "$APP_BUNDLE/Contents/Resources/assets"
if [ -d "$ROOT_DIR/tools" ]; then
    cp -R "$ROOT_DIR/tools" "$APP_BUNDLE/Contents/Resources/tools"
fi

if [ -f "$ICNS_PATH" ]; then
    cp "$ICNS_PATH" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
fi

cat > "$APP_BUNDLE/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>nanikaai</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.nanikaai.desktop</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>NanikaAi</string>
    <key>CFBundleDisplayName</key>
    <string>何か藍 (NanikaAi)</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.9</string>
    <key>CFBundleVersion</key>
    <string>0.1.9</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

echo "[4/4] Applying ad-hoc signature..."
codesign --force --deep --sign - "$APP_BUNDLE"

echo "[DONE] Built: $APP_BUNDLE"
du -sh "$APP_BUNDLE"
