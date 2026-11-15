# 何か藍 — NanikaAi (`nanikaai` `v1.2.2`)

> 🌐 **Language**: **[日本語 (Default)](#日本語)** | **[English (Default)](#english)** | **[한국어 (Optional)](README.ko.md)**

---

<a id="日本語"></a>

## 日本語 (Default)

### 名称の由来

デスクトップマスコット文化「伺か」の前身呼称である **`何か（なにか）`** に、象徴色の **`藍（あい = AI）`** と原作文化への **`愛（あい）`** を重ねて **`何か藍（なにかあい / NanikaAi）`** と命名した。

### 概要

**100% Safe Rust（`#![forbid(unsafe_code)]`）** で実装したマルチ OS 対応の AI デスクトップマスコット基盤（ベースウェア）である。
macOS・Windows・Linux 環境において、外部ランタイムの導入や OS ネイティブハンドルの `unsafe` 呼び出しを一切行わず、単一ネイティブバイナリとして動作する。

### 主な特徴

| 領域 | 仕様・実装内容 |
| :--- | :--- |
| **Zero `unsafe` ネイティブ GUI・Live2D 2.5D モーション** | `winit 0.30` + `wgpu 24` 透過アルファ合成（WGSL 2.5D 変形シェーダーによる呼吸・揺らぎ・カーソル視線追従・撫で/クリック弾性バウンス・発話時の揺れ＋SERIKO リップシンクを備え、RGB 色調は完全保持・メニューで切替可）と `cosmic-text 0.12` CJK 文字描画を採用している。`display.scale` 基準の HiDPI（`@2x`）表示、透過領域クリック透過、OS 別 UI 領域（macOS メニューバー／Dock、Windows タスクバー、X11／Wayland `_NET_WORKAREA`）の自動接地、33ms カーソル監視、日・英・韓 IME 対話欄（`Communicate`）およびトレイ統合を備える。 |
| **厳格なスコープ分離（`\0` / `\1`）** | Sakura Script の `\0`（メイン側）と `\1`（パートナー側）のサーフェス解決および会話履歴を完全に分離し、表示混線を防ぐ。既存の伺か `.nar` / `.zip` アーカイブのドラッグ＆ドロップおよびメニュー導入に対応する（`.png` / `.pna` / Shift-JIS `descript.txt`・`surfaces.txt` 互換）。 |
| **クリーンな資産分離（`MIT` + `CC0-1.0`）** | 同梱の既定スキン（`neo_default`、`clean_default`）は本プロジェクトで独自制作した **CC0-1.0** パブリックドメイン資産のみを含む。`\0`（正統派萌えテイストの紺白リボン衣装＋全身ブーツ `@2x`）、`\1`（鮮明なペリウィンクルブルー `#94A3FF` の丸もち体型と控えめな 2.5D 陰影・鼻先に小さく掛けた丸サングラスの上から両目が見えるデザイン `@2x`）、半透明パール調グラス吹き出し（`@2x`）で構成する。個人所有のクラシックシェル／バルーンは各 OS のユーザー領域から自動読み込みする。 |
| **マルチ AI 接続 & SSTP/HTTP 二重サーバー** | ローカル・リモート Ollama（`gemma4` 等）、Gemini CLI/API、Claude CLI/API、ChatGPT Codex、OpenAI 互換先に対応し、中国製 LLM 遮断保護を内蔵する。HTTP JSON（`127.0.0.1:9821`）とクラシック `SSTP/1.4` TCP（`127.0.0.1:9801`）を同時提供する。 |

### ユーザースキン（`.nar`）保存先

外部配布 `.nar` / `.zip` スキンをアプリ画面へドラッグ＆ドロップすると、下記ディレクトリへ自動配置される。

| OS | ユーザーデータディレクトリ |
| :--- | :--- |
| macOS | `~/Library/Application Support/NanikaAi/` |
| Windows | `%APPDATA%\NanikaAi\` |
| Linux | `$XDG_CONFIG_HOME/nanikaai/`（既定 `~/.config/nanikaai/`） |

### ビルドおよび起動方法

#### 前提条件

- Rust ツールチェーン（`cargo`, `rustc` 1.80+）
- Linux ビルド時は X11/Wayland/GTK トレイ開発パッケージ（`libgtk-3-dev`, `libxdo-dev` 等）

#### OS 別ビルド・実行

```bash
# macOS（.app バンドル作成と起動）
bash scripts/build_macos.sh
open "dist/NanikaAi.app"

# Linux
bash scripts/build_linux.sh
./dist/nanikaai-linux/nanikaai
```

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

#### 単体テストおよびスモーク検証

```bash
cargo test
cargo run --release -- --smoke-test
```

### HTTP / SSTP 連携 API

| 機能 | 接続先 / メソッド | リクエスト例 |
| :--- | :--- | :--- |
| 稼働確認 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 対話実行 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "今日の天気はどう？"}` |
| スクリプト発話 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]こんにちは！\\1\\s[10]よろしくな！\\e"}` |
| スキン切替 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| 一覧取得 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| クラシック `SSTP/1.4` | TCP `127.0.0.1:9801` | `SEND SSTP/1.4`（`Charset: UTF-8`, `Script: ...`） |

### ライセンス

- ソースコード: [MIT License](LICENSE)
- 同梱既定スキン（`CC0-1.0`）: [neo_default](assets/shells/neo_default/LICENSE-CC0.txt)、[clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

---

<a id="english"></a>

## English (Default)

### Origin of the Name

Named **`何か藍 (NanikaAi)`** by combining **`何か (Nanika)`** — the predecessor name of the classic desktop mascot culture *Ukagaka* (`伺か`) — with the signature indigo-blue theme **`藍 (Ai)`**, echoing both **AI** and affection (**`愛 / Ai`**) for the original culture.

### Overview

A **100% Safe Rust (`#![forbid(unsafe_code)]`)** cross-platform AI desktop mascot baseware.
It compiles into a single native binary for macOS, Windows, and Linux without external runtime dependencies or any `unsafe` OS handle calls.

### Key Features

| Area | Capabilities |
| :--- | :--- |
| **Zero `unsafe` Native GUI, Live2D 2.5D Motion & Snapping** | Built on `winit 0.30` + `wgpu 24` transparent compositing (with a Safe Rust WGSL 2.5D deformation shader for breathing, sway, cursor gaze parallax, jelly squish bounce on pat/click, and speaking bob with SERIKO `talk` lip-sync — preserving exact RGB colors and menu-toggleable) and `cosmic-text 0.12` CJK shaping. Supports HiDPI (`@2x`), click-through, OS work-area ground snapping, 33 ms cursor polling, multilingual IME (`Communicate`), and `tray-icon`. |
| **Strict Character Scope Isolation (`\0` / `\1`)** | Strictly isolates Sakura Script `\0` (main character) and `\1` (partner character) surface resolution and dialogue history to prevent cross-character mix-ups. Supports drag-and-drop and menu installation of classic `.nar` / `.zip` packages (`.png` / `.pna` / Shift-JIS `descript.txt` & `surfaces.txt`). |
| **Clean Open-Source Asset Separation (`MIT` + `CC0-1.0`)** | Bundled default skins (`neo_default`, `clean_default`) contain only original **CC0-1.0** public-domain assets created for this project: Sakura (`\0`) in a classic cute navy-and-white ribbon outfit with full-body boots (`@2x`), Unyu (`\1`) in vivid periwinkle blue (`#94A3FF`) with soft 2.5D shading, plump squishy paws, and small round sunglasses perched low on the nose with both eyes clearly visible above (`@2x`), plus a translucent pearl glassmorphism balloon (`@2x`). Classic personal shells load automatically from the OS user data directory. |
| **Multi-AI Backends & Dual SSTP/HTTP Server** | Connects to local/remote Ollama (`gemma4`, etc.), Gemini CLI/API, Claude CLI/API, ChatGPT Codex, and OpenAI-compatible endpoints with a built-in guard blocking restricted models. Exposes both HTTP JSON (`127.0.0.1:9821`) and classic `SSTP/1.4` TCP (`127.0.0.1:9801`). |

### User Skin (`.nar`) Data Directories

Drag and drop `.nar` or `.zip` skin archives onto the character window to install them into the OS-specific directory below.

| Operating System | User Data Directory |
| :--- | :--- |
| macOS | `~/Library/Application Support/NanikaAi/` |
| Windows | `%APPDATA%\NanikaAi\` |
| Linux | `$XDG_CONFIG_HOME/nanikaai/` (default `~/.config/nanikaai/`) |

### Build & Run

#### Prerequisites

- Rust toolchain (`cargo`, `rustc` 1.80+)
- On Linux: X11/Wayland/GTK tray development headers (`libgtk-3-dev`, `libxdo-dev`, etc.)

#### Platform Builds

```bash
# macOS (.app bundle)
bash scripts/build_macos.sh
open "dist/NanikaAi.app"

# Linux
bash scripts/build_linux.sh
./dist/nanikaai-linux/nanikaai
```

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

#### Tests & Smoke Verification

```bash
cargo test
cargo run --release -- --smoke-test
```

### HTTP / SSTP Integration API

| Action | Endpoint / Protocol | Example Payload / Command |
| :--- | :--- | :--- |
| Health check | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI chat | `POST http://127.0.0.1:9821/api/chat` | `{"message": "How is the weather today?"}` |
| Speak Sakura Script | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]Hello!\\1\\s[10]Nice to meet you!\\e"}` |
| Switch skin | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| List skins / providers | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| Classic TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

### License

- Source Code: [MIT License](LICENSE)
- Bundled Default Skins (`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

---

<details>
<summary>🇰🇷 한국어 안내 (Optional Korean Documentation)</summary>

- **100% Safe Rust (`#![forbid(unsafe_code)]`)** 기반 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어(macOS / Windows / Linux 단일 네이티브 바이너리)이다.
- `wgpu` WGSL 기반 **Live2D 스타일 2.5D GPU 모션**(원본 RGB 색상 무변경, 호흡·흔들림·시선 추적·쓰다듬기 탄성 바운스·SERIKO `talk` 립싱크, 메뉴 ON/OFF)과 **모에 사쿠라(`\0`) + 선명한 페리윙클 블루(`#94A3FF`) 말랑 우뉴(`\1`) `@2x` CC0-1.0 스킨**을 제공한다.
- **엄격한 `\0`/`\1` 스코프 분리**, `.nar`/`.zip`·Shift-JIS 호환 및 **멀티 AI 백엔드 + HTTP JSON (`127.0.0.1:9821`) · 클래식 `SSTP/1.4` (`127.0.0.1:9801`)** 듀얼 서버를 지원한다.
- 전체 한국어 안내 및 빌드·API 사용법은 **[README.ko.md](README.ko.md)** 에서 확인할 수 있다.

</details>
