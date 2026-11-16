# 何か藍 — NanikaAi (`nanikaai` `v0.1.3`)

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

### 主な特徴

| 領域 | 仕様・実装内容 |
| :--- | :--- |
| **Zero `unsafe` ネイティブ GUI・階層 Live2D リグ & Siri 風入力** | `winit 0.30` + `wgpu 24` 透過アルファ合成（足元完全固定・胸部呼吸・首支点アスペクト補正・2.5D 顔輪郭視差・髪/アホ毛/スカートの減衰バネ物理を備えた WGSL 階層リグと `\1` 専用もちリグを搭載、クラシックシェル向け軽量モード自動切替・RGB 色調完全保持・メニュー切替可）と `cosmic-text 0.12` CJK 文字描画を採用している。文字量連動で自動伸縮し上限超え時はマウスホイールで滑らかにスクロールできる吹き出し、サファイアブルー基調・点滅キャレット・IME 下線・送信/閉じるボタン付きの Siri 風 `Communicate` 入力バー（既定 UI 日・英表記）、スクロール可能な右クリックメニュー（最上段にワンクリック既定スキン復帰配置）、OS 別ワークエリア自動接地、トレイ統合を備える。 |
| **厳格なスコープ分離（`\0` / `\1`）** | Sakura Script の `\0`（メイン側）と `\1`（パートナー側）のサーフェス解決および会話履歴を完全に分離し、表示混線を防ぐ。既存の伺か `.nar` / `.zip` アーカイブのドラッグ＆ドロップおよびメニュー導入に対応し、`.png` / `.pna` アルファマスク合成、`seriko.use_self_alpha`、SERIKO `overlayf` / `replace` / `alternativestart`、Shift-JIS `descript.txt`・`surfaces.txt` との高い互換性を備える。 |
| **クリーンな資産分離（`MIT` + `CC0-1.0`）** | 同梱の既定スキン（`neo_default`、`clean_default`）は本プロジェクトで独自制作した **CC0-1.0** パブリックドメイン資産のみを含む。`\0`（正統派萌えテイストの紺白リボン衣装＋全身ブーツ、`330x675 pt` `@2x`）、`\1`（鮮明なペリウィンクルブルー `#94A3FF` の丸もち体型と控えめな 2.5D 陰影・鼻先に小さく掛けた丸サングラスの上から両目が見えるデザイン `@2x`）、半透明パール調グラス吹き出し（`@2x`）で構成する。個人所有のクラシックシェル／バルーンは各 OS のユーザー領域から自動読み込みする。 |
| **マルチ AI 接続 & SSTP/HTTP 二重サーバー** | ローカル・リモート Ollama（`gemma4` 等）、Gemini CLI/API、Claude CLI/API、ChatGPT Codex、OpenAI 互換先に対応し、中国製 LLM 遮断保護を内蔵する。HTTP JSON（`127.0.0.1:9821`）とクラシック `SSTP/1.4` TCP（`127.0.0.1:9801`）を同時提供する。 |

### ユーザースキン（`.nar`）保存先

外部配布 `.nar` / `.zip` スキンをアプリ画面へドラッグ＆ドロップすると、下記ディレクトリへ自動配置される。

| OS | ユーザーデータディレクトリ |
| :--- | :--- |
| macOS | `~/Library/Application Support/NanikaAi/` |
| Windows | `%APPDATA%\NanikaAi\` |
| Linux | `$XDG_CONFIG_HOME/nanikaai/`（既定 `~/.config/nanikaai/`） |

### 導入・インストール（初心者向けクイックスタート）

ソースコードのビルド環境（Rust 等）が不要な**配布実行ファイル**と、ローカル AI（**Ollama**）の構築まで AI エージェントに丸投げできる**ワンショット導入プロンプト**の両方を用意している。

#### 方法 1（推奨）: AI エージェント（`agy` / `claude` CLI 等）に全自動セットアップを任せる

手元の PC に **[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** または **Claude Code (`claude`)** などの AI コーディングエージェントを入れていれば、以下のコマンドまたはプロンプトを実行するだけで、実行ファイルの取得・Ollama の導入・PC メモリ容量に合った無料モデルのダウンロード・起動接続まで自動完了する。

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
| **2. Ollama と無料モデルを導入** | [Ollama 公式サイト](https://ollama.com/download) から Ollama を導入し、ターミナルで 1 つモデルを取得する（例: 16GB 環境 `ollama pull gemma4:e4b` または `ollama pull llama3.1:8b`、8GB 環境 `ollama pull llama3.2:3b`）。 |
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

# 単体テストおよびスモーク検証
cargo test
cargo run --release -- --smoke-test
```

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
| 一覧取得 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| クラシック `SSTP/1.4` | TCP `127.0.0.1:9801` | `SEND SSTP/1.4`（`Charset: UTF-8`, `Script: ...`） |

### 開発・保守体制（AI 開発）

- 本リポジトリは **AI 開発（AI-Developed & AI-Maintained）** プロジェクトであり、アーキテクチャ設計、`#![forbid(unsafe_code)]` 準拠の Rust 実装、同梱 `CC0-1.0` スキン資産の生成、自動単体・スモーク検証、および継続的な保守管理を AI エージェントが担う。
- すべての変更は `unsafe` コードゼロ、クリーンな権利分離（コード `MIT` ＋既定スキン `CC0-1.0`）、および自動テスト通過を必須条件として管理される。

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

### Key Features

| Area | Capabilities |
| :--- | :--- |
| **Zero `unsafe` Native GUI, Hierarchical Live2D Rig & Siri-Style Chat** | Built on `winit 0.30` + `wgpu 24` transparent compositing (Safe Rust WGSL hierarchical deformer rig with locked soles, localized chest breath, aspect-corrected neck pivot, 2.5D facial parallax, damped-spring hair/ahoge/cloth pendulums, dedicated Mochi mesh for `\1`, gentle auto-mode for classic shells, and all-expression SERIKO blink/`talk` lip-sync — preserving exact RGB colors and menu-toggleable) and `cosmic-text 0.12` CJK shaping. Features dynamically auto-sizing & scrollable speech balloons, a sapphire Siri-style `Communicate` glass bar with blinking caret, IME preedit underline, Send/Close buttons and Japanese/English default chrome, a scrollable context menu with 1-click default skin restore at the top, OS work-area snapping, and `tray-icon`. |
| **Strict Character Scope Isolation (`\0` / `\1`)** | Strictly isolates Sakura Script `\0` (main character) and `\1` (partner character) surface resolution and dialogue history to prevent cross-character mix-ups. Supports drag-and-drop and menu installation of classic `.nar` / `.zip` packages with `.png` / `.pna` alpha mask compositing, `seriko.use_self_alpha`, SERIKO `overlayf` / `replace` / `alternativestart`, and Shift-JIS `descript.txt` & `surfaces.txt` compatibility. |
| **Clean Open-Source Asset Separation (`MIT` + `CC0-1.0`)** | Bundled default skins (`neo_default`, `clean_default`) contain only original **CC0-1.0** public-domain assets created for this project: Sakura (`\0`) in a classic cute navy-and-white ribbon outfit with full-body boots (`330x675 pt` `@2x`), Unyu (`\1`) in vivid periwinkle blue (`#94A3FF`) with soft 2.5D shading, plump squishy paws, and small round sunglasses perched low on the nose with both eyes clearly visible above (`@2x`), plus a translucent pearl glassmorphism balloon (`@2x`). Classic personal shells load automatically from the OS user data directory. |
| **Multi-AI Backends & Dual SSTP/HTTP Server** | Connects to local/remote Ollama (`gemma4`, etc.), Gemini CLI/API, Claude CLI/API, ChatGPT Codex, and OpenAI-compatible endpoints with a built-in guard blocking restricted models. Exposes both HTTP JSON (`127.0.0.1:9821`) and classic `SSTP/1.4` TCP (`127.0.0.1:9801`). |

### User Skin (`.nar`) Data Directories

Drag and drop `.nar` or `.zip` skin archives onto the character window to install them into the OS-specific directory below.

| Operating System | User Data Directory |
| :--- | :--- |
| macOS | `~/Library/Application Support/NanikaAi/` |
| Windows | `%APPDATA%\NanikaAi\` |
| Linux | `$XDG_CONFIG_HOME/nanikaai/` (default `~/.config/nanikaai/`) |

### Installation & Beginner Quick Start

No compiler is needed to get started. We distribute **prebuilt executable archives** alongside a **one-shot AI agent setup prompt** so beginners can either delegate Ollama + local LLM installation to their coding CLI or finish manually in three steps.

#### Option 1 (Recommended): Let an AI Agent (`agy` / `claude` CLI) Set Everything Up

If you have **[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** or **Claude Code (`claude`)** installed, run a single command below (or paste the full prompt) to let your AI agent download the release binary, install **Ollama**, pick a free local model matching your RAM/VRAM, launch `NanikaAi`, and verify dialogue output automatically.

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
| **2. Install Ollama & Model** | Install [Ollama](https://ollama.com/download) and pull one model (e.g., 16 GB PCs: `ollama pull gemma4:e4b` or `ollama pull llama3.1:8b`; 8 GB PCs: `ollama pull llama3.2:3b`). |
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

# Unit tests & smoke verification
cargo test
cargo run --release -- --smoke-test
```

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
| List skins / providers | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| Classic TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

### Development & Maintenance (AI-Developed)

- This repository is an **AI-developed and AI-maintained** software project: architecture design, `#![forbid(unsafe_code)]` Rust source implementation, bundled `CC0-1.0` skin generation, automated unit/smoke verification, and ongoing maintenance are carried out autonomously by AI coding agents.
- Every change is governed by strict guardrails: zero `unsafe` blocks, clean open-source licensing (`MIT` code + `CC0-1.0` default skins), and automated test verification.

### License

- Source Code: [MIT License](LICENSE)
- Bundled Default Skins (`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

---

<details>
<summary>🇰🇷 한국어 안내 (Optional Korean Documentation)</summary>

- **AI 개발·유지보수 프로젝트 (AI-Developed & AI-Maintained)**: 아키텍처 설계부터 **100% Safe Rust (`#![forbid(unsafe_code)]`)** 소스 구현, `CC0-1.0` 기본 스킨 제작, 자동 검증 및 지속적인 저장소 유지보수까지 AI 에이전트가 자율적으로 전담한다.
- 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어로서 macOS / Windows / Linux 환경에서 외부 런타임 없이 단일 네이티브 바이너리로 동작한다.
- `wgpu` WGSL 기반 **계층형 Live2D 관절·감쇠 스프링 물리 리깅**(원본 RGB 색상 무변경, 발끝 고정·흉부 호흡·목 회전·시차·머리카락/바보털/치마 물리, SERIKO `talk` 립싱크)과 **모에 사쿠라(`\0`) + 선명한 페리윙클 블루(`#94A3FF`) 말랑 우뉴(`\1`) `@2x` 스킨**을 제공한다.
- 내용에 따라 자동 신축·스크롤되는 **유리 글래스 말풍선**과 점멸 커서·IME 밑줄·닫기/전송 버튼이 포함된 **사파이어 블루 Siri 스타일 입력 바**(기본 UI 일/영 병기)를 탑재했다.
- **엄격한 `\0`/`\1` 스코프 분리**, 원클릭 기본 스킨 복구·스크롤 메뉴, `.pna` 알파 마스크/`seriko.use_self_alpha`/SERIKO(`overlayf`·`replace`·`alternativestart`)·Shift-JIS 클래식 스킨 호환 및 **멀티 AI 백엔드 + HTTP JSON (`127.0.0.1:9821`) · 클래식 `SSTP/1.4` (`127.0.0.1:9801`)** 듀얼 서버를 지원한다.
- [GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest)에서 OS별 **실행 파일**을 내려받거나, `agy` / `claude` CLI 등 AI 에이전트에게 **원클릭 설치 프롬프트**를 지시해 **Ollama + 무료 로컬 모델(`gemma4`/`llama3`) 자동 설치 및 연결**까지 한 번에 마칠 수 있다.
- 전체 한국어 안내 및 AI 설치 프롬프트·API 사용법은 **[README.ko.md](README.ko.md)** 에서 확인할 수 있다.

</details>
