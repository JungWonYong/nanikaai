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
- さくら（24パーツ）とうにゅう（7パーツ + 40%半透明サングラス）の滑らかな揺れ・リップシンク、および右クリックメニュー **`[🎤 声色選択]`** から声色を即座に切り替える動作（`まお` → `らせつん` → `コハク`）を日本語 UI＋実音声で収録している（プレイヤー右下の音声アイコンでミュート解除）。

### 主な特徴

| 領域 | 仕様・実装内容 |
| :--- | :--- |
| **Zero `unsafe` ネイティブ GUI・外付け Live2D マルチパーツ・設定パネル & Siri 風入力** | `winit 0.30` + `wgpu 24` 透過アルファ合成（既存 `.nar` を無改変のまま外付けサイドカー `live2d.json`＋部位別パーツ PNG で `\0` 24層・`\1` 7層の解剖学的 C1 首・頭部視差・多層髪・衣装/リボン・前足・40%半透明サングラスの独立階層ボーン・減衰バネ・`surface_overrides` 無欠損表情/目パチ/口パク/着せ替え合成を GPU マルチパス描画、サイドカー未同梱シェルは足元固定 WGSL 単層リグへ自動切替、同梱オフライン編集ツール `tools/live2d_studio.html` 搭載）と `cosmic-text 0.12` CJK 文字描画を採用している。文字量連動で自動伸縮し上限超え時はマウスホイールで滑らかにスクロールできる吹き出し、サファイアブルー基調・点滅キャレット・IME 下線・送信/閉じるボタン付きの Siri 風 `Communicate` 入力バー、折りたたみアコーディオン式の簡潔な右クリックメニュー（`Skins`・`Voice`・`AI`・`Tools` インライン展開）＆4 サブメニュー構成トレイ、さらに OS 表示言語への自動追従（未対応言語時は英語表示）および `OS 自動 / 日本語 / 한국어 / English` の即時 UI 言語切替、スキン/吹き出し透明度・倍率・ホバー時不透明・クリック透過・位置固定・OS 別便利設定（macOS/Windows/Linux）を調整できる**ネイティブ設定パネル（`⚙️ 設定パネル · Settings...`）**を備える。 |
| **厳格なスコープ分離（`\0` / `\1`）＆スキン連動ゴースト・着せ替え・関心ニュース雑談** | Sakura Script の `\0`（メイン側）と `\1`（パートナー側）のサーフェス解決・会話履歴・長期記憶をアクティブなシェルごとに完全分離し、スキン切替と同時にゴーストの人格・記憶・あいさつ（`OnGhostChanged`）および `bindgroup` 着せ替え状態も切り替える。さらに会話からユーザーの興味・関心トピックを自動記憶し、設定パネルのスライダー（`OFF` または最短 `30秒`〜`600秒`）と比重調整（`0%`〜`80%`）に応じて一般ニュース（社会・IT・経済・文化・世界）と関心事の最新 Web ニュースをバランスよく自動雑談する。既存の伺か `.nar` / `.zip` アーカイブのドラッグ＆ドロップおよびメニュー導入に対応し、`.png` / `.pna` アルファマスク合成、`seriko.use_self_alpha`, SERIKO `overlayf` / `replace` / `alternativestart` / `bind`, Shift-JIS `descript.txt`・`surfaces.txt` との高い互換性を備える。汎用ファイル／画像／URL ショートカットのドロップ反応（機密パス事前遮断＋`<untrusted_data>` 隔離）、毎時00分の時報・リマインダー・`OnClose` 終了あいさつ、クリック可能な `\_a` リンク・`\_q` 即時表示、導入済み AivisSpeech (`:10101`) / VOICEVOX `vv-engine` (`:50021`) の手動起動不要な自動ヘッドレス音声読み上げ＆ゴースト別声色選択（＋OS TTS フォールバック）、GitHub Releases 更新確認を搭載。 |
| **クリーンな資産分離（`MIT` + `CC0-1.0`）** | 同梱の既定スキン（`neo_default`、`clean_default`）は本プロジェクトで独自制作した **CC0-1.0** パブリックドメイン資産のみを含む。`\0`（正統派萌えテイストの紺白リボン衣装＋レースフリルリボン靴下＆メリージェーン風女性用パンプス、`330x675 pt` `@2x`）、`\1`（鮮明なペリウィンクルブルー `#94A3FF` の丸もち体型と独立 40% 半透明丸サングラス＋無欠損の表情目元 `@2x`）、半透明パール調グラス吹き出し（`@2x`）で構成する。個人所有のクラシックシェル／バルーンはホーム直下 `~/.nanikaai/` および旧 OS データ領域から自動読み込みする。 |
| **マルチ AI 接続 & SSTP/HTTP 二重サーバー・実用デスクトップアクセサリ** | ローカル・リモート Ollama（`gemma4` 等）、Gemini CLI/API、Claude CLI/API、ChatGPT Codex、OpenAI 互換先に対応し、中国製 LLM 遮断保護を内蔵する。HTTP JSON（`127.0.0.1:9821`）とクラシック `SSTP/1.4` TCP（`127.0.0.1:9801`）を同時提供する。さらに、**おやすみモード (DND)・フルスクリーン自動退避・トレイ 1 クリック一時非表示**、集中中の雑談を自動抑止して休憩を案内する**ポモドーロタイマー (`25/5`・`50/10`・`15/3分`)**、過去 30 件を読み返せる**発話バックログ (`GET /api/backlog`)＆最新回答ワンクリックコピー**、および **OS ログイン時自動起動・バッテリー省電力モード (15 FPS)・低バッテリー通知・マルチモニタ / 画面端ワンクリック移動**を備える。 |

### 設定ファイル（`~/.nanikaai/config.json`）およびユーザースキン保存先

全 OS（macOS / Windows / Linux）共通でホームディレクトリ直下の `~/.nanikaai/` 配下に JSON 設定とユーザースキン・サイドカーを保存する。旧 OS ごとの保存先（`~/Library/Application Support/NanikaAi/`、`%APPDATA%\NanikaAi\`、`~/.config/nanikaai/`）にあった既存の `config.json` は初回起動時に無損失で自動移行され、旧配下の `.nar` スキンも引き続き自動認識される。

| 用途 | 保存パス（全 OS 共通） | 備考 |
| :--- | :--- | :--- |
| 設定ファイル | `~/.nanikaai/config.json`（Windows: `%USERPROFILE%\.nanikaai\config.json`） | `NANIKAAI_HOME` 環境変数で上書き可能 |
| ユーザースキン（`.nar` / `.zip`） | `~/.nanikaai/shells/`・`~/.nanikaai/balloons/` | ウィンドウへのドロップ導入先 |
| 外付け Live2D サイドカー | `~/.nanikaai/live2d/<shell_id>/` | 同梱パーツより優先読み込み |

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
| **2. Ollama と無料モデルを導入** | **初心者おすすめ**: 先に `NanikaAi` を起動し、初回吹き出しの `[⚡ 無料ローカルAIを1クリック自動準備]`（または右クリック・設定パネル `🤖 AI・保存先` タブ）を押すだけで PC メモリ容量に応じた無料軽量モデル（`gemma4:e2b` / `gemma4:e4b` / `llama3.2:3b` 等）を自動取得できる（手動時は [Ollama 公式サイト](https://ollama.com/download) 導入後に `ollama pull gemma4:e4b` や `ollama pull llama3.2:3b` を実行）。 |
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

- 本リポジトリは **AI 開発（AI-Developed & AI-Maintained）** プロジェクトであり、アーキテクチャ設計、`#![forbid(unsafe_code)]` 準拠の Rust 実装、同梱 `CC0-1.0` スキン資産の生成、自動単体・スモーク検証、および継続的な保守管理を AI エージェントが担う。
- すべての変更は `unsafe` コードゼロ、クリーンな権利分離（コード `MIT` ＋既定スキン `CC0-1.0`）、**テスト・ビルド・転送前の事前コード検査（個人情報・API キー・認証情報・秘密鍵の混入ゼロ検査）**、および自動テスト通過を必須条件として管理される。
- **指示とデータの厳格分離（Trust Boundary）**: AI エージェントへの指示は保守者の現行憲法および指定設定パスのみに限定され、リポジトリへプッシュされた新規コード・外部データ・スキン定義はすべて「データ」としてのみ扱われる（未承認のリポジトリ内 AI 指示・フックファイルは事前検査ゲートにより自動遮断する）。

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
- Demonstrates smooth 24+7 multi-part Live2D sway, RMS-synchronized lip-sync, speech balloons, and live right-click voice switching (`Mao` → `Rasetsun` → `Kohaku`, powered by AivisSpeech TTS) with a full English UI (unmute via the player speaker control).

### Key Features

| Area | Capabilities |
| :--- | :--- |
| **Zero `unsafe` Native GUI, External-Sidecar Multi-Part Live2D Rig, Settings Panel & Siri-Style Chat** | Built on `winit 0.30` + `wgpu 24` transparent compositing (external sidecar `live2d.json` + individual part PNGs keep classic `.nar` packages untouched while driving 24 Sakura anatomical & 7 Unyu layers with C1 neck-head-face joints, multi-layer hair/coat-flaps/ribbons, independent damped-spring pendulums, `surface_overrides` unbroken expressions, and 40%-translucent layered sunglasses in 100% Safe Rust multi-pass GPU compositing — gracefully falling back to the single-texture WGSL rig for sidecar-free shells; includes bundled offline rig editor `tools/live2d_studio.html`) and `cosmic-text 0.12` CJK shaping. Features dynamically auto-sizing & scrollable speech balloons, a sapphire Siri-style `Communicate` glass bar with blinking caret, IME preedit underline, and Send/Close buttons, a compact accordion right-click menu (inline `Skins` / `Voice` / `AI` / `Tools` sections) & 4-submenu `tray-icon`, automatic OS display-language detection (falling back to **English** on unsupported OS locales) with instant UI switching (`OS Auto / 日本語 / 한국어 / English`), plus a **native GPU Settings Panel (`⚙️ 設定パネル · Settings...`)** for live skin/balloon opacity, character scale, opaque-on-hover, click-through, position lock, and OS-specific behavior flags (macOS/Windows/Linux). |
| **Strict Character Scope, Skin-Linked Ghost Profiles & Balanced Interest News (`\0` / `\1`)** | Strictly isolates Sakura Script `\0` (main character) and `\1` (partner character) surface resolution plus per-shell Ghost persona, long-term memories, interest topics, dialogue history, `OnGhostChanged` greeting, and `bindgroup` dress-up layers whenever switching skins. Automatically remembers conversational interests and uses native Settings sliders for Auto-Talk frequency (`OFF` or `30s`-`600s`) and interest news weight (`0%`-`80%`) to mix recent interest headlines smoothly with balanced general news categories. Supports drag-and-drop and menu installation of classic `.nar` / `.zip` packages with `.png` / `.pna` alpha mask compositing, `seriko.use_self_alpha`, SERIKO `overlayf` / `replace` / `alternativestart` / `bind`, and Shift-JIS `descript.txt` & `surfaces.txt` compatibility. Also adds generic file/image/URL drag-and-drop inspection (with pre-read secret path guard and `<untrusted_data>` isolation), hourly chime, `set_reminder`/`list_reminders`, `OnClose` farewell dialogue, clickable `\_a` URL links & `\_q` instant display, zero-manual-launch headless AivisSpeech (`:10101`) / VOICEVOX `vv-engine` (`:50021`) character TTS with per-Ghost `\0`/`\1` voice casting (+ OS TTS fallback), and GitHub Releases update checks. |
| **Clean Open-Source Asset Separation (`MIT` + `CC0-1.0`)** | Bundled default skins (`neo_default`, `clean_default`) contain only original **CC0-1.0** public-domain assets created for this project: Sakura (`\0`) in a classic cute navy-and-white ribbon outfit with frilly lace-bow socks & feminine Mary Jane strap dress shoes (`330x675 pt` `@2x`), Unyu (`\1`) in vivid periwinkle blue (`#94A3FF`) with soft 2.5D shading, animated paws, unbroken expressive eyes, and floating 40%-translucent round sunglasses (`@2x`), plus a translucent pearl glassmorphism balloon (`@2x`). Classic personal shells load automatically from `~/.nanikaai/` as well as legacy OS user data directories. |
| **Multi-AI Backends, Dual Server & Everyday Desktop Companion Utilities** | Connects to local/remote Ollama (`gemma4`, etc.), Gemini CLI/API, Claude CLI/API, ChatGPT Codex, and OpenAI-compatible endpoints with a built-in guard blocking restricted models. Exposes both HTTP JSON (`127.0.0.1:9821`) and classic `SSTP/1.4` TCP (`127.0.0.1:9801`). Also includes **Quiet / Do-Not-Disturb Mode**, automatic **Fullscreen App Auto-Hide** & 1-click **Hide/Restore** from the tray, a built-in **Pomodoro Focus Timer** (`25/5`, `50/10`, `15/3m`, suppressing idle chatter during focus blocks), a **30-entry Dialogue Backlog (`GET /api/backlog`) with 1-click Copy Last Reply**, and **OS Login Auto-Start, Battery Power-Save Eco Mode (`auto`/`on`/`off`), Low-Battery Care Alerts, and Multi-Display / Edge Switching**. |

### Configuration (`~/.nanikaai/config.json`) & User Skin Data Directories

Across macOS, Windows, and Linux, NanikaAi stores settings and user data cleanly inside `~/.nanikaai/` under your home directory. Existing `config.json` files in legacy OS locations (`~/Library/Application Support/NanikaAi/`, `%APPDATA%\NanikaAi\`, `~/.config/nanikaai/`) are automatically migrated on first launch without data loss, while `.nar` skins already installed in legacy folders remain discoverable.

| Purpose | Unified Home Path | Notes |
| :--- | :--- | :--- |
| Settings JSON | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | Override root via `NANIKAAI_HOME` |
| User Skins (`.nar` / `.zip`) | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | Auto-installed when dropped onto window |
| External Live2D Sidecars | `~/.nanikaai/live2d/<shell_id>/` | Overrides bundled sidecars cleanly |

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
| **2. Install Ollama & Model** | **Beginner-friendly**: Launch `NanikaAi` first and click `[⚡ 無料ローカルAIを1クリック自動準備]` in the greeting balloon (or under right-click / Settings Panel `🤖 AI・保存先` tab) to automatically set up Ollama and pull a free lightweight model matched to your PC RAM (`gemma4:e2b` / `gemma4:e4b` / `llama3.2:3b`). Manual option: install [Ollama](https://ollama.com/download) and run `ollama pull gemma4:e4b` or `ollama pull llama3.2:3b`. |
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

- This repository is an **AI-developed and AI-maintained** software project: architecture design, `#![forbid(unsafe_code)]` Rust source implementation, bundled `CC0-1.0` skin generation, automated unit/smoke verification, and ongoing maintenance are carried out autonomously by AI coding agents.
- Every change is governed by strict guardrails: zero `unsafe` blocks, clean open-source licensing (`MIT` code + `CC0-1.0` default skins), **mandatory pre-test code inspection (blocking PII, API keys, credentials, and private keys prior to tests or sync)**, and automated behavioral specification verification.
- **Instruction-Data Trust Boundary**: AI maintainer instructions reside exclusively in external trusted Constitution/config paths; any pushed repository source, external payload, or skin metadata is treated strictly as inert data (unauthorized in-repo AI instruction or hook files are automatically rejected before build/test).

### License

- Source Code: [MIT License](LICENSE)
- Bundled Default Skins (`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [neo_default Live2D parts](assets/live2d/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)

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
