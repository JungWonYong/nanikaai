# 何か藍 — NanikaAi (`nanikaai` `v0.1.10`)

> 🌐 **Language**: **[日本語 (Default)](#日本語)** | **[English (Default)](#english)** | **[한국어 (Optional)](README.ko.md)**
>
> 🤖 **AI-Developed & AI-Maintained / AI開発・AI保守プロジェクト**: 本ソフトウェアの設計・`100% Safe Rust` 実装・`CC0-1.0` 既定スキン制作・検証・継続保守は AI エージェントによって自律的に行われている。 / Designed, implemented in `100% Safe Rust`, verified, and continuously maintained by AI coding agents.

---

<a id="日本語"></a>

## 日本語 (Default)

### 名称の由来

デスクトップマスコット文化「伺か」の前身呼称である **`何か（なにか）`** に、象徴色の **`藍（あい = AI）`** と原作文化への **`愛（あい）`** を重ねて **`何か藍（なにかあい / NanikaAi）`** と命名した。

### 概要

AI エージェントによる自律開発・継続保守で構築された、**100% Safe Rust（`#![forbid(unsafe_code)]`）** 製のマルチ OS 対応 AI デスクトップマスコット基盤（ベースウェア）である。
macOS・Windows・Linux 環境において、外部ランタイムの導入や OS ネイティブハンドルの `unsafe` 呼び出しを一切行わず、単一ネイティブバイナリとして動作する。

### 🎬 デモ映像（日本語版 / Live2D モーション＆AivisSpeech 実音声）

https://github.com/user-attachments/assets/f998f5e0-aee3-4114-b922-84622a7aadc8

- 🌐 **[専用紹介ページ (`GitHub Pages · 日本語/EN/KO 切替`)](https://jungwonyong.github.io/nanikaai/?lang=ja)** · 📥 MP4: **[日本語版 (`nanikaai_intro_ja.mp4`)](docs/media/nanikaai_intro_ja.mp4)** / **[English (`nanikaai_intro_en.mp4`)](docs/media/nanikaai_intro_en.mp4)** / **[한국어 (`nanikaai_intro_ko.mp4`)](docs/media/nanikaai_intro_ko.mp4)**
- さくら（24層）・うにゅう（7層＋半透明サングラス）の Live2D モーションと、右クリックメニューから声色を切り替える様子を実音声付きで収録（プレイヤー右下で音声オン可）。

### 主な特徴

- **100% Safe Rust ネイティブ GUI**: `winit` + `wgpu` 透過描画の単一バイナリ（macOS / Windows / Linux・外部ランタイム不要）。
- **外付け Live2D マルチパーツ**: `live2d.json` + 部位 PNG でさくら 24 層・うにゅう 7 層の揺れ・目パチ・口パク・着せ替え（`bindgroup`）を描画（編集ツール `tools/live2d_studio.html` 同梱）。
- **設定パネル & 多言語 UI**: 透明度・倍率・クリック透過・位置固定・OS 別挙動を即時調整。OS 言語に自動追従し、`OS 自動 / 日本語 / 한국어 / English` を切替可能（未対応言語は英語）。
- **伺かスキン互換**: `.nar` / `.zip` のドロップ導入、`\0`/`\1` スコープ分離、シェル別の人格・記憶・着せ替え切替に対応。
- **音声読み上げ & 関心ニュース雑談**: AivisSpeech / VOICEVOX を自動連携（声色切替対応）。会話から関心事を記憶し、設定間隔（`OFF` / `30秒`–`600秒`）と比重（`0%`–`80%`）で一般ニュースと混ぜて自動雑談。
- **デスクトップアクセサリ**: おやすみモード（DND）・フルスクリーン自動退避、ポモドーロタイマー（`25/5`・`50/10`・`15/3分`）、過去 30 件の発話バックログ＆ワンクリックコピー、自動起動・省電力モード・画面端移動。
- **マルチ AI & デュアル API**: Ollama（初回 1 クリック自動準備対応）・Gemini・Claude・ChatGPT Codex・OpenAI 互換先に対応し、HTTP JSON（`:9821`）と `SSTP/1.4`（`:9801`）を同時提供。
- **ライセンス分離**: ソースコード `MIT`＋独自制作の同梱スキン（`neo_default` / `clean_default`）は `CC0-1.0`。

### 設定ファイル（`~/.nanikaai/config.json`）およびユーザースキン保存先

全 OS 共通でホーム配下の `~/.nanikaai/` に保存する（旧 OS 固有パスの既存 `config.json` は初回起動時に自動移行し、旧 `.nar` も継続認識）。

| 用途 | 保存パス（全 OS 共通） | 備考 |
| :--- | :--- | :--- |
| 設定ファイル | `~/.nanikaai/config.json`（Windows: `%USERPROFILE%\.nanikaai\config.json`） | `NANIKAAI_HOME` 環境変数で上書き可能 |
| ユーザースキン（`.nar` / `.zip`） | `~/.nanikaai/shells/`・`~/.nanikaai/balloons/` | ウィンドウへのドロップ導入先 |
| 外付け Live2D サイドカー | `~/.nanikaai/live2d/<shell_id>/` | 同梱パーツより優先読み込み |

### 導入・インストール（初心者向けクイックスタート）

Rust 環境不要の**配布実行ファイル**と、AI エージェントに丸投げできる**自動導入プロンプト**を用意している。

#### 方法 1（推奨）: AI エージェント（`agy` / `claude` CLI 等）に全自動セットアップを任せる

**[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** や **Claude Code (`claude`)** があれば、以下のコマンド 1 行で導入から無料モデル接続・動作確認まで自動完了する。

**ターミナル 1 行実行例（`agy` または `claude` 導入後）:**

```bash
# Google Antigravity CLI (`agy`) を使う場合
agy -i "https://github.com/JungWonYong/nanikaai の最新リリース実行ファイルとローカルOllamaをこのPCに導入・起動し、スペックに合う無料モデル(gemma4/llama3系)を接続して動作確認まで完了して。"

# Claude Code CLI (`claude`) を使う場合
claude "https://github.com/JungWonYong/nanikaai の最新リリース実行ファイルとローカルOllamaをこのPCに導入・起動し、スペックに合う無料モデル(gemma4/llama3系)を接続して動作確認まで完了して。"
```

<details>
<summary>📋 AI エージェント用・完全セットアップ指示プロンプト（クリックで展開・コピペ用）</summary>

```text
このPCにデスクトップマスコット「何か藍 (NanikaAi)」とローカルAI（Ollama）を導入し、すぐ会話できる状態までセットアップしてください。

1. OS（macOS / Windows / Linux）とCPUアーキテクチャ、搭載RAM/GPU VRAMを確認する。
2. https://github.com/JungWonYong/nanikaai/releases/latest から該当OS用のビルド済みアーカイブ（NanikaAi-macos-arm64.zip / NanikaAi-windows-x64.zip / NanikaAi-linux-x64.tar.gz）を取得し、適切な場所（macOSは ~/Applications/NanikaAi.app と xattr -cr 実行、Windowsは %LOCALAPPDATA%\NanikaAi、Linuxは ~/.local/share/nanikaai）へ展開する。該当アーキテクチャの配布物がない場合のみソースをビルドする。
3. Ollama が未導入なら公式手段（macOS: brew install ollama または ollama.com、Windows: winget install Ollama.Ollama、Linux: 公式インストール手順）で導入し、http://127.0.0.1:11434 で ollama serve が起動している状態にする。
4. PCのメモリ容量に合わせて、安全な無料モデル（gemma4 または llama3 系）を1つ `ollama pull` する（目安: 8GBなら `llama3.2:3b` または `gemma4:e2b`、16GBなら `gemma4:e4b` または `llama3.1:8b`、32GB以上なら `gemma4:12b` または `gemma4:31b`）。※制限対象モデルは使用しないこと。
5. NanikaAi を起動し、`GET http://127.0.0.1:9821/api/health` が応答するまで待機したあと、`POST http://127.0.0.1:9821/api/provider` に `{"provider":"ollama","model":"auto"}` を送信して接続し、`POST http://127.0.0.1:9821/api/speak` に `{"script":"\\0\\s[5]セットアップ完了だよ！\\1\\s[10]いつでも話しかけてな！\\e"}` を送って動作を確認し、配置パスと使用モデル名を報告する。
```

</details>

> 💡 **`agy` / `claude` CLI を直接会話バックエンドにする場合**: ローカル GPU / Ollama を使わなくても、`agy` または `claude` CLI がインストール・認証済みであれば、キャラクター右クリックメニューの `[AI プロバイダー · Providers]` からそのまま呼び出して会話できる。

#### 方法 2: 実行ファイル＋Ollama で手動導入（3 ステップ・コンパイラ不要）

| ステップ | 作業内容 |
| :--- | :--- |
| **1. アプリ本体を取得・展開** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) から OS 別の配布ファイル（`NanikaAi-macos-arm64.zip` / `NanikaAi-windows-x64.zip` / `NanikaAi-linux-x64.tar.gz`）をダウンロードして展開する（macOS 初回起動時に警告が出た場合は `xattr -cr NanikaAi.app` を実行）。 |
| **2. Ollama と無料モデルを導入** | 初回吹き出し（または設定パネル `🤖 AI・保存先`）の `[⚡ 無料ローカルAIを1クリック自動準備]` で PC スペックに合うモデル（`gemma4:e2b` / `llama3.2:3b` 等）を自動取得（手動時は [Ollama](https://ollama.com/download) 導入後 `ollama pull gemma4:e4b`）。 |
| **3. 起動して会話開始** | `NanikaAi` を起動すると既定設定のままローカル Ollama（`http://127.0.0.1:11434` / `model: "auto"`）へ自動接続する。さくらをダブルクリックすると `Communicate` 入力バーが開き、そのまま会話できる。 |

<details>
<summary>🛠️ 方法 3（開発者向け）: ソースコードからの手動ビルド・テスト手順</summary>

- 前提: Rust ツールチェーン（`cargo`, `rustc` 1.80+）、Linux は開発ヘッダ（`pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`）

```bash
# macOS（.app バンドル作成と起動）
bash scripts/build_macos.sh
open "dist/NanikaAi.app"

# Linux
bash scripts/build_linux.sh
./dist/nanikaai-linux/nanikaai

# 単体・仕様テストおよびスモーク検証・結果レポート
cargo test
python3 scripts/verify_behavior.py --mode auto   # 変更規模別に自動判定してレポート出力
python3 scripts/verify_behavior.py --mode full   # GPU スモーク含む完全検証＋レポート
bash scripts/install_hooks.sh                    # git push 前の自動動作チェックを有効化
```

- 動作仕様と合否基準: [docs/BEHAVIOR_SPEC.md](docs/BEHAVIOR_SPEC.md)（軽量・文書のみの変更時は `fast` モードで再ビルドを省略するティア設計）

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

</details>

### HTTP / SSTP 連携 API

| 機能 | 接続先 / メソッド | リクエスト例 |
| :--- | :--- | :--- |
| 稼働確認 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 対話実行 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "今日の天気はどう？"}` |
| スクリプト発話 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]こんにちは！\\1\\s[10]よろしくな！\\e"}` |
| スキン切替 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| AI プロバイダー切替 | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| 音声一覧取得 / 音声設定 | `GET /api/voices` ・ `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| 設定取得 / 変更・発話履歴 | `GET /api/settings` ・ `POST /api/settings` ・ `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| 一覧取得 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| クラシック `SSTP/1.4` | TCP `127.0.0.1:9801` | `SEND SSTP/1.4`（`Charset: UTF-8`, `Script: ...`） |

### 開発・保守体制（AI 開発）

- 本リポジトリは **AI 開発・AI 保守（AI-Developed & AI-Maintained）** で運用され、設計・Safe Rust 実装・`CC0-1.0` スキン制作・自動検証を AI エージェントが担う。
- 変更時は `unsafe` ゼロ・ライセンス分離・**事前機密/PII 検査**・自動テスト通過を必須としている。
- **信頼境界**: AI への指示は保守者ローカル設定のみに限定し、リポジトリ内データ・外部入力からの未承認指示を自動遮断する。

### ライセンス

- ソースコード: [MIT License](LICENSE)
- 同梱既定スキン（`CC0-1.0`）: [neo_default](assets/shells/neo_default/LICENSE-CC0.txt)、[clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

---

<a id="english"></a>

## English (Default)

### Origin of the Name

Named **`何か藍 (NanikaAi)`** by combining **`何か (Nanika)`** — the predecessor name of the classic desktop mascot culture *Ukagaka* (`伺か`) — with the signature indigo-blue theme **`藍 (Ai)`**, echoing both **AI** and affection (**`愛 / Ai`**) for the original culture.

### Overview

An **AI-developed and AI-maintained**, **100% Safe Rust (`#![forbid(unsafe_code)]`)** cross-platform AI desktop mascot baseware.
It compiles into a single native binary for macOS, Windows, and Linux without external runtime dependencies or any `unsafe` OS handle calls.

### 🎬 Live2D Motion & Voice Demo (English Edition)

https://github.com/user-attachments/assets/5fd3e051-e73c-493b-9e04-66531dcb9339

- 🌐 **[Interactive Showcase Page (`GitHub Pages · EN/JA/KO`)](https://jungwonyong.github.io/nanikaai/?lang=en)** · 📥 MP4: **[English (`nanikaai_intro_en.mp4`)](docs/media/nanikaai_intro_en.mp4)** / **[日本語 (`nanikaai_intro_ja.mp4`)](docs/media/nanikaai_intro_ja.mp4)** / **[한국어 (`nanikaai_intro_ko.mp4`)](docs/media/nanikaai_intro_ko.mp4)**
- Features 24-layer Sakura & 7-layer Unyu Live2D motion plus live right-click voice switching with authentic AivisSpeech TTS (unmute via the player speaker button).

### Key Features

- **100% Safe Rust Native GUI**: Single `winit` + `wgpu` transparent native binary for macOS, Windows, and Linux with zero external runtimes.
- **External-Sidecar Multi-Part Live2D**: Renders 24 Sakura and 7 Unyu anatomical layers (`live2d.json` + PNGs) for sway, blink, lip-sync, and `bindgroup` dress-up (includes offline editor `tools/live2d_studio.html`).
- **Settings Panel & Multilingual UI**: Adjust opacity, scale, click-through, position lock, and OS behavior live; tracks OS language automatically (`OS Auto / 日本語 / 한국어 / English`, English fallback on unsupported locales).
- **Ukagaka Skin Compatibility**: Drag-and-drop `.nar` / `.zip` installs with strict `\0`/`\1` isolation and per-shell Ghost persona, memory, and dress-up switching.
- **TTS Voice & Interest-Balanced News**: Headless AivisSpeech / VOICEVOX playback with voice casting, plus conversational interest memory mixed with general news at configurable intervals (`OFF` or `30s`–`600s`, `0%`–`80%` weight).
- **Desktop Companion Utilities**: Quiet / Do-Not-Disturb & fullscreen auto-hide, Pomodoro focus timer (`25/5`, `50/10`, `15/3m`), 30-entry dialogue backlog with 1-click copy, login auto-start, battery eco mode, and edge snap.
- **Multi-AI & Dual Server**: Connects Ollama (with 1-click beginner auto-setup), Gemini, Claude, ChatGPT Codex, and OpenAI-compatible endpoints while exposing HTTP JSON (`:9821`) and `SSTP/1.4` (`:9801`).
- **Clean License Separation**: `MIT` source code paired with original `CC0-1.0` default skins (`neo_default` / `clean_default`).

### Configuration (`~/.nanikaai/config.json`) & User Skin Data Directories

Settings JSON and user skins live uniformly under `~/.nanikaai/` across all OSes (legacy OS `config.json` files migrate automatically on first launch while legacy `.nar` skins stay readable).

| Purpose | Unified Home Path | Notes |
| :--- | :--- | :--- |
| Settings JSON | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | Override root via `NANIKAAI_HOME` |
| User Skins (`.nar` / `.zip`) | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | Auto-installed when dropped onto window |
| External Live2D Sidecars | `~/.nanikaai/live2d/<shell_id>/` | Overrides bundled sidecars cleanly |

### Installation & Beginner Quick Start

Both **prebuilt binaries** (no Rust required) and a **one-shot AI agent setup prompt** are included.

#### Option 1 (Recommended): Let an AI Agent (`agy` / `claude` CLI) Set Everything Up

With **[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** or **Claude Code (`claude`)** installed, a single command below installs, binds a free local model, and verifies speech automatically:

**One-Line CLI Launch (after installing `agy` or `claude`):**

```bash
# Using Google Antigravity CLI (`agy`)
agy -i "Install the latest release binary of https://github.com/JungWonYong/nanikaai and local Ollama on this machine, pull a free gemma4/llama3 model matching my PC specs, bind them, and verify it works."

# Using Claude Code CLI (`claude`)
claude "Install the latest release binary of https://github.com/JungWonYong/nanikaai and local Ollama on this machine, pull a free gemma4/llama3 model matching my PC specs, bind them, and verify it works."
```

<details>
<summary>📋 Full Copy-Paste Setup Prompt for Any AI Agent (Antigravity / Claude Code / Cursor / Codex)</summary>

```text
Please install and configure the "NanikaAi (何か藍)" desktop mascot and local Ollama LLM on this machine so I can start chatting immediately:

1. Check my OS (macOS / Windows / Linux), CPU architecture, system RAM, and GPU VRAM.
2. Download the matching prebuilt release archive (`NanikaAi-macos-arm64.zip`, `NanikaAi-windows-x64.zip`, or `NanikaAi-linux-x64.tar.gz`) from https://github.com/JungWonYong/nanikaai/releases/latest and extract it to a clean app folder (macOS: `~/Applications/NanikaAi.app` and clear quarantine with `xattr -cr ~/Applications/NanikaAi.app`; Windows: `%LOCALAPPDATA%\NanikaAi`; Linux: `~/.local/share/nanikaai`). Only build from source if no matching archive exists for this architecture.
3. If `ollama` is not installed yet, install it cleanly (`brew install ollama` or ollama.com on macOS, `winget install Ollama.Ollama` on Windows, or the official Linux installer) and ensure `ollama serve` is listening on `http://127.0.0.1:11434`.
4. Choose and `ollama pull` one free non-restricted `gemma4` or `llama3` model that fits available memory:
   - ~8 GB RAM: `llama3.2:3b` or `gemma4:e2b`
   - ~16 GB RAM: `gemma4:e4b` or `llama3.1:8b`
   - 32 GB+ RAM: `gemma4:12b` or `gemma4:31b`
5. Launch `NanikaAi`, wait for `GET http://127.0.0.1:9821/api/health` to return `ok`, call `POST http://127.0.0.1:9821/api/provider` with `{"provider":"ollama","model":"auto"}`, trigger a greeting via `POST http://127.0.0.1:9821/api/speak` with `{"script":"\\0\\s[5]Setup complete!\\1\\s[10]Ready when you are!\\e"}`, and report the install path and active model.
```

</details>

> 💡 **Using `agy` or `claude` directly without a local GPU**: When `agy` or `claude` CLI is installed and signed in on your machine, `NanikaAi` also detects it automatically under right-click `[AI プロバイダー · Providers]` so you can chat right away without downloading local LLM weights.

#### Option 2: Prebuilt Binary + Ollama in 3 Manual Steps (No Rust Required)

| Step | Instructions |
| :--- | :--- |
| **1. Download App Bundle** | Download your OS package (`NanikaAi-macos-arm64.zip`, `NanikaAi-windows-x64.zip`, or `NanikaAi-linux-x64.tar.gz`) from [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) and extract it (on macOS, run `xattr -cr NanikaAi.app` once if Gatekeeper prompts). |
| **2. Install Ollama & Model** | Click `[⚡ 無料ローカルAIを1クリック自動準備]` in the welcome balloon (or Settings `🤖 AI・保存先`) to auto-setup Ollama and pull a RAM-matched model (`gemma4:e2b`, `llama3.2:3b`, etc.), or manually install [Ollama](https://ollama.com/download) and run `ollama pull gemma4:e4b`. |
| **3. Launch & Chat** | Start `NanikaAi` — default settings automatically connect to local Ollama (`http://127.0.0.1:11434`, `model: "auto"`). Double-click Sakura to open the chat bar. |

<details>
<summary>🛠️ Option 3 (Developers): Build from Source & Run Tests</summary>

- Prerequisites: Rust toolchain (`cargo`, `rustc` 1.80+); on Linux: `pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`

```bash
# macOS (.app bundle)
bash scripts/build_macos.sh
open "dist/NanikaAi.app"

# Linux
bash scripts/build_linux.sh
./dist/nanikaai-linux/nanikaai

# Unit / Spec tests & automated verification report
cargo test
python3 scripts/verify_behavior.py --mode auto   # Auto-selects tier & prints report
python3 scripts/verify_behavior.py --mode full   # Full cargo test + GPU smoke report
bash scripts/install_hooks.sh                    # Enable automatic pre-push checks
```

- Specification & pass criteria: [docs/BEHAVIOR_SPEC.md](docs/BEHAVIOR_SPEC.md) (trivial doc-only edits automatically run the fast static tier without recompiling Rust)

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

</details>

### HTTP / SSTP Integration API

| Action | Endpoint / Protocol | Example Payload / Command |
| :--- | :--- | :--- |
| Health check | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI chat | `POST http://127.0.0.1:9821/api/chat` | `{"message": "How is the weather today?"}` |
| Speak Sakura Script | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]Hello!\\1\\s[10]Nice to meet you!\\e"}` |
| Switch skin | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| Switch AI provider | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| Voices / Configure TTS | `GET /api/voices` & `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| Settings / Backlog API | `GET /api/settings` & `POST /api/settings` & `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| List skins / providers | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| Classic TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

### Development & Maintenance (AI-Developed)

- This project is **AI-developed and AI-maintained**: architecture, `#![forbid(unsafe_code)]` Rust code, `CC0-1.0` skins, automated verification, and maintenance are handled autonomously by AI agents.
- Every update must pass zero `unsafe` checks, `MIT`/`CC0-1.0` asset separation, **pre-test secret/PII scanning**, and automated behavioral tests.
- **Trust Boundary**: Agent instructions reside exclusively in external trusted config paths; repo inputs and unapproved in-repo instruction/hook files are treated strictly as untrusted data.

### License

- Source Code: [MIT License](LICENSE)
- Bundled Default Skins (`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [neo_default Live2D parts](assets/live2d/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

---

<details>
<summary>🇰🇷 한국어 안내 (Optional Korean Documentation)</summary>

- **100% Safe Rust & 외장 Live2D**: `winit` + `wgpu` 투명 네이티브 GUI와 사쿠라(24층)·우뉴(7층) 멀티파츠 Live2D, `.nar` 스킨 호환(`\0`/`\1` 분리)을 단일 바이너리로 제공한다.
- **설정 패널·다국어·음성·잡담**: 투명도·크기·OS별 동작·UI 언어(`OS 자동 / 日本語 / 한국어 / English`) 조절, 헤드리스 AivisSpeech/VOICEVOX 음성, 관심사 기억 기반 뉴스 잡담을 지원한다.
- **데스크톱 편의 기능 & 듀얼 API**: 방해 금지·전체 화면 자동 숨김·뽀모도로 타이머·발화 백로그(30건)·자동 시작/절전 모드 및 HTTP(`:9821`)·`SSTP/1.4`(`:9801`)를 탑재했다.
- 전체 한국어 가이드와 AI 자동 설치·API 문서는 **[README.ko.md](README.ko.md)** 에서 확인할 수 있다.

</details>
