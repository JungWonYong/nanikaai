# 何か藍 (NanikaAi) — 機能・設定・導入ガイド

> 🏠 **[トップ (README.md)](../README.md)** · 🌐 **日本語** | **[English](guide-en.md)** | **[한국어](guide-ko.md)** · 🔌 **[API](API.md)**

## 1. 主な機能

- **100% Safe Rust ネイティブ GUI**: `winit` + `wgpu` 透過描画の単一バイナリ（macOS / Windows / Linux・外部ランタイム不要）。
- **外付け Live2D マルチパーツ**: `live2d.json` + 部位 PNG でさくら 24 層・うにゅう 7 層の揺れ・目パチ・口パク・着せ替え（`bindgroup`）を描画（編集ツール `tools/live2d_studio.html` 同梱）。
- **設定パネル & 多言語 UI**: 透明度・倍率・クリック透過・位置固定・OS 別挙動を即時調整。OS 言語へ自動追従し、`OS 自動 / 日本語 / 한국어 / English` を切替可能（未対応言語は英語）。
- **伺かスキン互換**: `.nar` / `.zip` のドロップ導入、`\0`/`\1` スコープ分離、シェル別の人格・記憶・着せ替え切替に対応。
- **音声読み上げ & 関心ニュース雑談**: AivisSpeech / VOICEVOX を自動連携（声色切替対応）。会話から関心事を記憶し、設定間隔（`OFF` / `30秒`–`600秒`）と比重（`0%`–`80%`）で一般ニュースと混ぜて自動雑談。
- **デスクトップアクセサリ**: おやすみモード（DND）・フルスクリーン自動退避、ポモドーロタイマー（`25/5`・`50/10`・`15/3分`）、過去 30 件の発話バックログ＆ワンクリックコピー、自動起動・省電力モード・画面端移動。
- **マルチ AI & デュアル API**: Ollama（初回 1 クリック自動準備対応）・Gemini・Claude・ChatGPT Codex・OpenAI 互換先に対応し、HTTP JSON（`:9821`）と `SSTP/1.4`（`:9801`）を同時提供。

## 2. 設定ファイル・スキン保存先（全 OS 共通 `~/.nanikaai/`）

旧 OS 固有パスの既存 `config.json` は初回起動時に自動移行され、旧 `.nar` スキンも引き続き認識される。

| 用途 | 保存パス（全 OS 共通） | 備考 |
| :--- | :--- | :--- |
| 設定ファイル | `~/.nanikaai/config.json`（Windows: `%USERPROFILE%\.nanikaai\config.json`） | `NANIKAAI_HOME` 環境変数で上書き可能 |
| ユーザースキン（`.nar` / `.zip`） | `~/.nanikaai/shells/`・`~/.nanikaai/balloons/` | ウィンドウへのドロップ導入先 |
| 外付け Live2D サイドカー | `~/.nanikaai/live2d/<shell_id>/` | 同梱パーツより優先読み込み |

## 3. 手動導入（3 ステップ）

| ステップ | 作業内容 |
| :--- | :--- |
| **1. アプリ本体を取得** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) から OS 別アーカイブを展開（macOS 初回警告時は `xattr -cr NanikaAi.app`）。 |
| **2. Ollama と無料モデルを導入** | 初回吹き出し（または設定パネル `🤖 AI・保存先`）の `[⚡ 無料ローカルAIを1クリック自動準備]` で自動取得（手動時は [Ollama](https://ollama.com/download) 導入後 `ollama pull gemma4:e4b`）。 |
| **3. 起動して会話開始** | 既定設定のままローカル Ollama（`http://127.0.0.1:11434` / `model: "auto"`）へ自動接続。さくらをダブルクリックして会話開始。 |

## 4. ソースビルド・テスト（開発者向け）

- 前提: Rust 1.80+（Linux は `pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`）

```bash
bash scripts/build_macos.sh && open "dist/NanikaAi.app"       # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # 検証
```

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

## 5. AI 開発・保守体制

- 設計・`#![forbid(unsafe_code)]` Rust 実装・`CC0-1.0` スキン制作・自動検証を AI エージェントが自律運用する。
- 変更時は `unsafe` ゼロ・ライセンス分離・**事前機密/PII 検査**・自動テスト通過を必須とし、未承認のリポジトリ内 AI 指示・フックは事前遮断する。
