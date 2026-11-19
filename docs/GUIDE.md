# 何か藍 (NanikaAi) — 詳細ガイド / Full Guide / 상세 가이드

> 🌐 **Language**: **[日本語](#日本語)** | **[English](#english)** | **[한국어](#한국어)** · 🏠 **[トップ (README.md)](../README.md)** · **[한국어 README](../README.ko.md)** · **[動作仕様書 (BEHAVIOR_SPEC.md)](BEHAVIOR_SPEC.md)**

---

<a id="日本語"></a>

## 日本語

### 1. 主な機能

- **100% Safe Rust ネイティブ GUI**: `winit` + `wgpu` 透過描画の単一バイナリ（macOS / Windows / Linux・外部ランタイム不要）。
- **外付け Live2D マルチパーツ**: `live2d.json` + 部位 PNG でさくら 24 層・うにゅう 7 層の揺れ・目パチ・口パク・着せ替え（`bindgroup`）を描画（編集ツール `tools/live2d_studio.html` 同梱）。
- **設定パネル & 多言語 UI**: 透明度・倍率・クリック透過・位置固定・OS 別挙動を即時調整。OS 言語へ自動追従し、`OS 自動 / 日本語 / 한국어 / English` を切替可能（未対応言語は英語）。
- **伺かスキン互換**: `.nar` / `.zip` のドロップ導入、`\0`/`\1` スコープ分離、シェル別の人格・記憶・着せ替え切替に対応。
- **音声読み上げ & 関心ニュース雑談**: AivisSpeech / VOICEVOX を自動連携（声色切替対応）。会話から関心事を記憶し、設定間隔（`OFF` / `30秒`–`600秒`）と比重（`0%`–`80%`）で一般ニュースと混ぜて自動雑談。
- **デスクトップアクセサリ**: おやすみモード（DND）・フルスクリーン自動退避、ポモドーロタイマー（`25/5`・`50/10`・`15/3分`）、過去 30 件の発話バックログ＆ワンクリックコピー、自動起動・省電力モード・画面端移動。
- **マルチ AI & デュアル API**: Ollama（初回 1 クリック自動準備対応）・Gemini・Claude・ChatGPT Codex・OpenAI 互換先に対応し、HTTP JSON（`:9821`）と `SSTP/1.4`（`:9801`）を同時提供。
- **ライセンス分離**: ソースコード `MIT`＋独自制作の同梱スキン（`neo_default` / `clean_default`）は `CC0-1.0`。

### 2. 設定ファイル・スキン保存先（全 OS 共通 `~/.nanikaai/`）

旧 OS 固有パスの既存 `config.json` は初回起動時に自動移行され、旧 `.nar` スキンも引き続き認識される。

| 用途 | 保存パス（全 OS 共通） | 備考 |
| :--- | :--- | :--- |
| 設定ファイル | `~/.nanikaai/config.json`（Windows: `%USERPROFILE%\.nanikaai\config.json`） | `NANIKAAI_HOME` 環境変数で上書き可能 |
| ユーザースキン（`.nar` / `.zip`） | `~/.nanikaai/shells/`・`~/.nanikaai/balloons/` | ウィンドウへのドロップ導入先 |
| 外付け Live2D サイドカー | `~/.nanikaai/live2d/<shell_id>/` | 同梱パーツより優先読み込み |

### 3. 導入手順（手動 3 ステップ / ソースビルド）

| ステップ | 作業内容 |
| :--- | :--- |
| **1. アプリ本体を取得** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) から OS 別アーカイブを展開（macOS 初回警告時は `xattr -cr NanikaAi.app`）。 |
| **2. Ollama と無料モデルを導入** | 初回吹き出し（または設定パネル `🤖 AI・保存先`）の `[⚡ 無料ローカルAIを1クリック自動準備]` で自動取得（手動時は [Ollama](https://ollama.com/download) 導入後 `ollama pull gemma4:e4b`）。 |
| **3. 起動して会話開始** | 既定設定のままローカル Ollama（`http://127.0.0.1:11434` / `model: "auto"`）へ自動接続。さくらをダブルクリックして会話開始。 |

```bash
# ソースビルド（Rust 1.80+）
bash scripts/build_macos.sh && open "dist/NanikaAi.app"      # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # 検証
```

### 4. HTTP / SSTP 連携 API

| 機能 | 接続先 / メソッド | リクエスト例 |
| :--- | :--- | :--- |
| 稼働確認 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 対話実行 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "今日の天気はどう？"}` |
| スクリプト発話 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]こんにちは！\\1\\s[10]よろしくな！\\e"}` |
| スキン切替 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| AI プロバイダー切替 | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| 音声一覧 / 設定 | `GET /api/voices` ・ `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| 設定 / 発話履歴 | `GET/POST /api/settings` ・ `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| 一覧取得 | `GET /api/skins` ・ `GET /api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| クラシック `SSTP/1.4` | TCP `127.0.0.1:9801` | `SEND SSTP/1.4`（`Charset: UTF-8`, `Script: ...`） |

---

<a id="english"></a>

## English

### 1. Key Features

- **100% Safe Rust Native GUI**: Single `winit` + `wgpu` transparent native binary for macOS, Windows, and Linux with zero external runtimes.
- **External-Sidecar Multi-Part Live2D**: Renders 24 Sakura and 7 Unyu anatomical layers (`live2d.json` + PNGs) for sway, blink, lip-sync, and `bindgroup` dress-up (includes offline editor `tools/live2d_studio.html`).
- **Settings Panel & Multilingual UI**: Adjust opacity, scale, click-through, position lock, and OS behavior live; tracks OS language automatically (`OS Auto / 日本語 / 한국어 / English`, English fallback on unsupported locales).
- **Ukagaka Skin Compatibility**: Drag-and-drop `.nar` / `.zip` installs with strict `\0`/`\1` isolation and per-shell Ghost persona, memory, and dress-up switching.
- **TTS Voice & Interest-Balanced News**: Headless AivisSpeech / VOICEVOX playback with voice casting, plus conversational interest memory mixed with general news at configurable intervals (`OFF` or `30s`–`600s`, `0%`–`80%` weight).
- **Desktop Companion Utilities**: Quiet / Do-Not-Disturb & fullscreen auto-hide, Pomodoro focus timer (`25/5`, `50/10`, `15/3m`), 30-entry dialogue backlog with 1-click copy, login auto-start, battery eco mode, and edge snap.
- **Multi-AI & Dual Server**: Connects Ollama (with 1-click beginner auto-setup), Gemini, Claude, ChatGPT Codex, and OpenAI-compatible endpoints while exposing HTTP JSON (`:9821`) and `SSTP/1.4` (`:9801`).
- **Clean License Separation**: `MIT` source code paired with original `CC0-1.0` default skins (`neo_default` / `clean_default`).

### 2. Configuration & User Skin Directories (`~/.nanikaai/`)

Legacy OS `config.json` files migrate automatically on first launch while legacy `.nar` skins stay readable.

| Purpose | Unified Home Path | Notes |
| :--- | :--- | :--- |
| Settings JSON | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | Override root via `NANIKAAI_HOME` |
| User Skins (`.nar` / `.zip`) | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | Auto-installed when dropped onto window |
| External Live2D Sidecars | `~/.nanikaai/live2d/<shell_id>/` | Overrides bundled sidecars cleanly |

### 3. Manual Install & Source Build

| Step | Instructions |
| :--- | :--- |
| **1. Download App Bundle** | Extract your OS package from [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) (on macOS, run `xattr -cr NanikaAi.app` if prompted). |
| **2. Install Ollama & Model** | Click `[⚡ 無料ローカルAIを1クリック自動準備]` in the welcome balloon (or Settings `🤖 AI・保存先`), or install [Ollama](https://ollama.com/download) and run `ollama pull gemma4:e4b`. |
| **3. Launch & Chat** | Default settings auto-connect to local Ollama (`http://127.0.0.1:11434`, `model: "auto"`). Double-click Sakura to chat. |

```bash
# Build from source (Rust 1.80+)
bash scripts/build_macos.sh && open "dist/NanikaAi.app"      # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # Verify
```

### 4. HTTP / SSTP Integration API

| Action | Endpoint / Protocol | Example Payload / Command |
| :--- | :--- | :--- |
| Health check | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI chat | `POST http://127.0.0.1:9821/api/chat` | `{"message": "How is the weather today?"}` |
| Speak Sakura Script | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]Hello!\\1\\s[10]Nice to meet you!\\e"}` |
| Switch skin | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| Switch AI provider | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| Voices / TTS | `GET /api/voices` & `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| Settings / Backlog | `GET/POST /api/settings` & `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| List skins / providers | `GET /api/skins` & `GET /api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| Classic TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

---

<a id="한국어"></a>

## 한국어

### 1. 주요 기능

- **100% Safe Rust 네이티브 GUI**: `winit` + `wgpu` 투명 렌더링 기반 단일 바이너리(macOS / Windows / Linux·외부 런타임 불필요).
- **외장 사이드카 Live2D 멀티파츠**: `live2d.json` + 부위별 PNG로 사쿠라 24층·우뉴 7층 스웨이·눈깜빡임·립싱크·갈아입히기(`bindgroup`) 구현(오프라인 편집 도구 `tools/live2d_studio.html` 동봉).
- **설정 패널 & 다국어 UI**: 투명도·배율·클릭 스루·위치 잠금·OS별 동작 즉시 조절. OS 언어 자동 추적 및 `OS 자동 / 日本語 / 한국어 / English` 전환 지원(미지원 언어는 영어 표시).
- **우카가카 스킨 호환**: `.nar` / `.zip` 드래그 앤 드롭 설치, `\0`/`\1` 스코프 분리, 셸 단위 고스트 인격·기억·의상 전환 지원.
- **음성 합성 & 관심 뉴스 잡담**: AivisSpeech / VOICEVOX 자동 연동(캐릭터별 음색 선택). 대화 속 관심사를 기억해 설정 간격(`OFF` / `30초`–`600초`)·비중(`0%`–`80%`)에 맞춰 일반 뉴스와 균형 있게 잡담.
- **데스크톱 편의 액세서리**: 방해 금지(DND)·전체 화면 자동 숨김, 뽀모도로 타이머(`25/5`·`50/10`·`15/3분`), 최근 30건 발화 백로그 및 원클릭 복사, 로그인 자동 시작·절전 모드·화면 구석 이동.
- **멀티 AI & 듀얼 서버**: Ollama(초보자 1클릭 자동 준비 포함)·Gemini·Claude·ChatGPT Codex·OpenAI 호환 지원, HTTP JSON(`:9821`)과 `SSTP/1.4`(`:9801`) 동시 제공.
- **라이선스 분리**: 소스 코드 `MIT` + 자체 제작 기본 스킨(`neo_default` / `clean_default`) `CC0-1.0`.

### 2. 설정 파일 및 스킨 저장 경로 (`~/.nanikaai/`)

기존 OS별 경로의 `config.json`은 첫 실행 시 자동 이관되며 기존 `.nar` 스킨도 계속 인식한다.

| 항목 | 저장 경로 (전 OS 통일) | 비고 |
| :--- | :--- | :--- |
| 설정 JSON (`config.json`) | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | `NANIKAAI_HOME` 환경 변수로 재지정 가능 |
| 사용자 스킨·말풍선 | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | `.nar` / `.zip` 드래그 앤 드롭 시 자동 설치 |
| 외장 Live2D 사이드카 | `~/.nanikaai/live2d/<shell_id>/` | 내장 사이드카보다 우선 로드 |

### 3. 수동 설치 및 소스 빌드

| 단계 | 진행 내용 |
| :--- | :--- |
| **1. 실행 파일 다운로드** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest)에서 OS별 아카이브 압축 해제(macOS 경고 시 `xattr -cr NanikaAi.app`). |
| **2. Ollama 및 무료 모델 설치** | 첫 말풍선(또는 설정 패널 `🤖 AI・保存先`)의 `[⚡ 無料ローカルAIを1クリック自動準備]` 클릭(수동 시 [Ollama](https://ollama.com/download) 설치 후 `ollama pull gemma4:e4b`). |
| **3. 앱 실행 및 대화** | 기본 설정 그대로 로컬 Ollama(`http://127.0.0.1:11434`, `model: "auto"`)에 자동 연결. 사쿠라를 더블클릭해 대화 시작. |

```bash
# 소스 빌드 (Rust 1.80+)
bash scripts/build_macos.sh && open "dist/NanikaAi.app"      # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # 검증
```

### 4. HTTP / SSTP 연동 API

| 구분 | 엔드포인트 / 프로토콜 | 요청 예시 |
| :--- | :--- | :--- |
| 상태 확인 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 대화 요청 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "오늘 날씨 어때?"}` |
| 사쿠라 스크립트 발화 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]안녕!\\1\\s[10]반가워!\\e"}` |
| 스킨 전환 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| AI 프로바이더 전환 | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| 음성 목록 / 설정 | `GET /api/voices` · `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| 설정 / 백로그 | `GET/POST /api/settings` · `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| 목록 조회 | `GET /api/skins` · `GET /api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| 클래식 TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |
