#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

DIST_DIR="$ROOT_DIR/dist/nanikaai-linux"
BIN_PATH="$ROOT_DIR/target/release/nanikaai"

echo "[1/3] Building release binary (100% Safe Rust)..."
cargo build --release

echo "[2/3] Assembling portable Linux bundle..."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"
cp "$BIN_PATH" "$DIST_DIR/nanikaai"
chmod +x "$DIST_DIR/nanikaai"
cp -R "$ROOT_DIR/assets" "$DIST_DIR/assets"
if [ -d "$ROOT_DIR/tools" ]; then
    cp -R "$ROOT_DIR/tools" "$DIST_DIR/tools"
fi
cp "$ROOT_DIR/LICENSE" "$DIST_DIR/LICENSE"

cat > "$DIST_DIR/nanikaai.desktop" << 'DESKTOP'
[Desktop Entry]
Version=1.0
Type=Application
Name=何か藍 (NanikaAi)
Name[ja]=何か藍 (NanikaAi)
Name[en]=NanikaAi (何か藍)
Name[ko]=何か藍 (NanikaAi)
Comment=100% Safe Rust Cross-Platform AI Desktop Mascot Baseware
Comment[ja]=100% Safe Rust製のクロスプラットフォームAIデスクトップマスコット基盤
Comment[ko]=100% Safe Rust 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어
Exec=nanikaai
Icon=icon
Terminal=false
Categories=Utility;
DESKTOP

echo "[3/3] Bundle ready at $DIST_DIR"
du -sh "$DIST_DIR"
