# 何か藍 — NanikaAi (`v0.1.10`)

> 🌐 **Language**: **[日本語 (Default)](#日本語)** | **[English (Default)](#english)** | **[한국어 (README.ko.md)](README.ko.md)** · 🎬 **[Demo Page](https://jungwonyong.github.io/nanikaai/)** · 📖 **[詳細ガイド / Full Guide](docs/GUIDE.md)**

---

<a id="日本語"></a>

## 日本語 (Default)

**100% Safe Rust（`#![forbid(unsafe_code)]`）** で AI エージェントが自律開発・保守する、macOS / Windows / Linux 対応の「伺か」系 AI デスクトップマスコット基盤（ベースウェア）。
前身呼称「**何か**」に象徴色「**藍（AI / 愛）**」を重ねて命名。

- 📥 **[最新リリース (GitHub Releases)](https://github.com/JungWonYong/nanikaai/releases/latest)** · 📖 **[詳細ガイド (`docs/GUIDE.md`)](docs/GUIDE.md#日本語)** · 📋 **[動作仕様 (`docs/BEHAVIOR_SPEC.md`)](docs/BEHAVIOR_SPEC.md)**

### 🎬 デモ映像（日本語版）

https://github.com/user-attachments/assets/f998f5e0-aee3-4114-b922-84622a7aadc8

- 🌐 **[切替デモページ (GitHub Pages)](https://jungwonyong.github.io/nanikaai/?lang=ja)** · 📥 MP4: **[日本語](docs/media/nanikaai_intro_ja.mp4)** / **[English](docs/media/nanikaai_intro_en.mp4)** / **[한국어](docs/media/nanikaai_intro_ko.mp4)**（プレイヤー右下で音声オン可）

### 🚀 クイックスタート

- **実行ファイル**: [GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest) から展開し、初回吹き出しの `[⚡ 無料ローカルAIを1クリック自動準備]` を押すだけで開始できる。
- **AI エージェント自動導入（`agy` / `claude`）**:

```bash
agy -i "https://github.com/JungWonYong/nanikaai の最新リリース実行ファイルとローカルOllamaをこのPCに導入・起動し、スペックに合う無料モデル(gemma4/llama3系)を接続して動作確認まで完了して。"
```

<details>
<summary>✨ 主な機能一覧（クリックで展開）</summary>

- **100% Safe Rust ネイティブ GUI**: `winit` + `wgpu` 透過描画の単一バイナリ（macOS / Windows / Linux・外部ランタイム不要）。
- **外付け Live2D マルチパーツ**: `live2d.json` + 部位 PNG でさくら 24 層・うにゅう 7 層の揺れ・目パチ・口パク・着せ替え（`bindgroup`）を描画（編集ツール `tools/live2d_studio.html` 同梱）。
- **設定パネル & 多言語 UI**: 透明度・倍率・クリック透過・位置固定・OS 別挙動を即時調整。OS 言語へ自動追従し、`OS 自動 / 日本語 / 한국어 / English` を切替可能（未対応言語は英語）。
- **伺かスキン互換**: `.nar` / `.zip` のドロップ導入、`\0`/`\1` スコープ分離、シェル別の人格・記憶・着せ替え切替に対応。
- **音声読み上げ & 関心ニュース雑談**: AivisSpeech / VOICEVOX を自動連携（声色切替対応）。会話から関心事を記憶し、設定間隔（`OFF` / `30秒`–`600秒`）と比重（`0%`–`80%`）で一般ニュースと混ぜて自動雑談。
- **デスクトップアクセサリ**: おやすみモード（DND）・フルスクリーン自動退避、ポモドーロタイマー（`25/5`・`50/10`・`15/3分`）、過去 30 件の発話バックログ＆ワンクリックコピー、自動起動・省電力モード・画面端移動。
- **マルチ AI & デュアル API**: Ollama・Gemini・Claude・ChatGPT Codex・OpenAI 互換先に対応し、HTTP JSON（`:9821`）と `SSTP/1.4`（`:9801`）を同時提供。
- **ライセンス分離**: ソースコード `MIT`＋独自制作の同梱スキン（`neo_default` / `clean_default`）は `CC0-1.0`。

</details>

<details>
<summary>⚙️ 設定保存先（~/.nanikaai/）・手動導入・ソースビルド</summary>

- **保存先（全 OS 共通）**: 設定 `~/.nanikaai/config.json`、スキン `~/.nanikaai/shells/`・`~/.nanikaai/balloons/`、外付け Live2D `~/.nanikaai/live2d/<shell_id>/`（旧 OS 固有パスから自動移行・読み込み対応）。
- **手動 3 ステップ・ソースビルド・AI 導入用詳細プロンプト**: **[詳細ガイド (`docs/GUIDE.md`)](docs/GUIDE.md#日本語)** を参照。

</details>

<details>
<summary>🔌 HTTP / SSTP API・AI 保守体制・ライセンス</summary>

- **API**: HTTP JSON（`127.0.0.1:9821`：`/api/health`, `/api/chat`, `/api/speak`, `/api/skin`, `/api/provider`, `/api/voice`, `/api/settings`, `/api/backlog`）および `SSTP/1.4`（`127.0.0.1:9801`）。詳細は **[docs/GUIDE.md](docs/GUIDE.md#日本語)**。
- **AI 開発・保守体制**: 設計・Safe Rust 実装・`CC0-1.0` スキン制作・事前機密/PII 検査・自動検証を AI エージェントが自律運用（未承認のリポジトリ内指示・フックは事前遮断）。
- **ライセンス**: ソースコード [MIT License](LICENSE) / 同梱スキン（`CC0-1.0`）[neo_default](assets/shells/neo_default/LICENSE-CC0.txt)・[clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)。

</details>

---

<a id="english"></a>

## English (Default)

An **AI-developed and AI-maintained**, **100% Safe Rust (`#![forbid(unsafe_code)]`)** cross-platform AI desktop mascot baseware inspired by *Ukagaka* (`伺か`) for macOS, Windows, and Linux.

- 📥 **[Latest Release (GitHub Releases)](https://github.com/JungWonYong/nanikaai/releases/latest)** · 📖 **[Full Guide (`docs/GUIDE.md`)](docs/GUIDE.md#english)** · 📋 **[Behavior Spec (`docs/BEHAVIOR_SPEC.md`)](docs/BEHAVIOR_SPEC.md)**

### 🎬 Live2D & Voice Demo (English Edition)

https://github.com/user-attachments/assets/5fd3e051-e73c-493b-9e04-66531dcb9339

- 🌐 **[Interactive Demo Page (GitHub Pages)](https://jungwonyong.github.io/nanikaai/?lang=en)** · 📥 MP4: **[English](docs/media/nanikaai_intro_en.mp4)** / **[日本語](docs/media/nanikaai_intro_ja.mp4)** / **[한국어](docs/media/nanikaai_intro_ko.mp4)** (unmute via the speaker icon)

### 🚀 Quick Start

- **Prebuilt Binary**: Extract from [GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest) and click `[⚡ 無料ローカルAIを1クリック自動準備]` in the welcome balloon.
- **One-Line AI Agent Setup (`agy` / `claude`)**:

```bash
agy -i "Install the latest release binary of https://github.com/JungWonYong/nanikaai and local Ollama on this machine, pull a free gemma4/llama3 model matching my PC specs, bind them, and verify it works."
```

<details>
<summary>✨ Key Features (Click to expand)</summary>

- **100% Safe Rust Native GUI**: Single `winit` + `wgpu` transparent native binary for macOS, Windows, and Linux with zero external runtimes.
- **External-Sidecar Multi-Part Live2D**: Renders 24 Sakura and 7 Unyu anatomical layers (`live2d.json` + PNGs) for sway, blink, lip-sync, and `bindgroup` dress-up (includes offline editor `tools/live2d_studio.html`).
- **Settings Panel & Multilingual UI**: Adjust opacity, scale, click-through, position lock, and OS behavior live; tracks OS language automatically (`OS Auto / 日本語 / 한국어 / English`, English fallback on unsupported locales).
- **Ukagaka Skin Compatibility**: Drag-and-drop `.nar` / `.zip` installs with strict `\0`/`\1` isolation and per-shell Ghost persona, memory, and dress-up switching.
- **TTS Voice & Interest-Balanced News**: Headless AivisSpeech / VOICEVOX playback with voice casting, plus conversational interest memory mixed with general news at configurable intervals (`OFF` or `30s`–`600s`, `0%`–`80%` weight).
- **Desktop Companion Utilities**: Quiet / Do-Not-Disturb & fullscreen auto-hide, Pomodoro focus timer (`25/5`, `50/10`, `15/3m`), 30-entry dialogue backlog with 1-click copy, login auto-start, battery eco mode, and edge snap.
- **Multi-AI & Dual Server**: Connects Ollama, Gemini, Claude, ChatGPT Codex, and OpenAI-compatible endpoints while exposing HTTP JSON (`:9821`) and `SSTP/1.4` (`:9801`).
- **Clean License Separation**: `MIT` source code paired with original `CC0-1.0` default skins (`neo_default` / `clean_default`).

</details>

<details>
<summary>⚙️ Config Paths (~/.nanikaai/), Manual Install & Source Build</summary>

- **Unified Home Paths**: Settings `~/.nanikaai/config.json`, Skins `~/.nanikaai/shells/` & `~/.nanikaai/balloons/`, Sidecars `~/.nanikaai/live2d/<shell_id>/` (auto-migrates legacy OS configs).
- **Step-by-step Install & Rust Build Commands**: See **[Full Guide (`docs/GUIDE.md`)](docs/GUIDE.md#english)**.

</details>

<details>
<summary>🔌 HTTP / SSTP API, AI Maintenance & License</summary>

- **API**: HTTP JSON (`127.0.0.1:9821`: `/api/health`, `/api/chat`, `/api/speak`, `/api/skin`, `/api/provider`, `/api/voice`, `/api/settings`, `/api/backlog`) and `SSTP/1.4` (`127.0.0.1:9801`). Details in **[docs/GUIDE.md](docs/GUIDE.md#english)**.
- **AI Maintenance & Trust Boundary**: Autonomously developed and verified with zero `unsafe` blocks, mandatory pre-test PII/secret scanning, and strict separation of trusted maintainer instructions from repo data.
- **License**: Source [MIT License](LICENSE) / Default Skins (`CC0-1.0`) [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [neo_default Live2D parts](assets/live2d/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt).

</details>

---

> 🇰🇷 **한국어 문서**: 간단한 소개 및 한국어 데모 영상은 **[README.ko.md](README.ko.md)**, 상세 기능·API 가이드는 **[docs/GUIDE.md](docs/GUIDE.md#한국어)** 에서 확인할 수 있다.
