# 何か藍 — NanikaAi (`nanikaai` `v0.1.10`)

> 🌐 **Language**: **[日本語 (Default)](README.md#日本語)** | **[English (Default)](README.md#english)** | **한국어 (Optional)**
>
> 🤖 **AI 개발·유지보수 프로젝트 (AI-Developed & AI-Maintained)**: 본 소프트웨어의 설계, `100% Safe Rust` 구현, `CC0-1.0` 기본 스킨 제작, 자동 검증 및 지속적인 저장소 관리는 AI 에이전트가 자율적으로 수행한다.

---

## 1. 명칭의 유래

우카가카(`伺か`)의 전신 명칭인 **`何か(나니카)`**에 시그니처 블루 테마 **`藍(쪽빛 남 · 훈독 あい = AI)`** 및 원작 문화에 대한 **`愛`**를 겹쳐 **`何か藍 / なにかあい (NanikaAi)`**로 명명했다.

## 2. 개요

AI 에이전트가 자율적으로 개발·유지보수하는 **100% Safe Rust (`#![forbid(unsafe_code)]`)** 기반 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어(Baseware)이다.
macOS, Windows, Linux 환경에서 외부 런타임 설치나 OS 네이티브 핸들 `unsafe` 호출 없이 단일 네이티브 바이너리로 동작한다.

## 2-1. 🎬 소개 동영상 (한국어판 / Live2D 모션 & 음성 발화 데모)

https://github.com/user-attachments/assets/a9e7e117-35c1-4a71-a4b2-c1e5234186ba

- 🌐 **[소개 랜딩 페이지 (`GitHub Pages · KO/JA/EN 전환`)](https://jungwonyong.github.io/nanikaai/?lang=ko)** · 📥 MP4: **[한국어판 (`nanikaai_intro_ko.mp4`)](docs/media/nanikaai_intro_ko.mp4)** / **[日本語 (`nanikaai_intro_ja.mp4`)](docs/media/nanikaai_intro_ja.mp4)** / **[English (`nanikaai_intro_en.mp4`)](docs/media/nanikaai_intro_en.mp4)**
- 사쿠라(24레이어)·우뉴(7레이어＋반투명 선글라스)의 Live2D 모션과 우클릭 메뉴 실시간 목소리 전환을 실제 음성과 함께 수록(우측 하단 스피커 아이콘으로 음소거 해제).

## 3. 주요 특징

- **100% Safe Rust 네이티브 GUI**: `winit` + `wgpu` 투명 렌더링 기반 단일 바이너리(macOS / Windows / Linux·외부 런타임 불필요).
- **외장 사이드카 Live2D 멀티파츠**: `live2d.json` + 부위별 PNG로 사쿠라 24층·우뉴 7층 스웨이·눈깜빡임·립싱크·갈아입히기(`bindgroup`) 구현(오프라인 편집 도구 `tools/live2d_studio.html` 동봉).
- **설정 패널 & 다국어 UI**: 투명도·배율·클릭 스루·위치 잠금·OS별 동작 즉시 조절. OS 언어 자동 추적 및 `OS 자동 / 日本語 / 한국어 / English` 전환 지원(미지원 언어는 영어 표시).
- **우카가카 스킨 호환**: `.nar` / `.zip` 드래그 앤 드롭 설치, `\0`/`\1` 스코프 분리, 셸 단위 고스트 인격·기억·의상 전환 지원.
- **음성 합성 & 관심 뉴스 잡담**: AivisSpeech / VOICEVOX 자동 연동(캐릭터별 음색 선택). 대화 속 관심사를 기억해 설정 간격(`OFF` / `30초`–`600초`)·비중(`0%`–`80%`)에 맞춰 일반 뉴스와 균형 있게 잡담.
- **데스크톱 편의 액세서리**: 방해 금지(DND)·전체 화면 자동 숨김, 뽀모도로 타이머(`25/5`·`50/10`·`15/3분`), 최근 30건 발화 백로그 및 원클릭 복사, 로그인 자동 시작·절전 모드·화면 구석 이동.
- **멀티 AI & 듀얼 서버**: Ollama(초보자 1클릭 자동 준비 포함)·Gemini·Claude·ChatGPT Codex·OpenAI 호환 지원, HTTP JSON(`:9821`)과 `SSTP/1.4`(`:9801`) 동시 제공.
- **라이선스 분리**: 소스 코드 `MIT` + 자체 제작 기본 스킨(`neo_default` / `clean_default`) `CC0-1.0`.

## 4. 설정 파일(`~/.nanikaai/config.json`) 및 사용자 스킨(`.nar`) 저장 경로

모든 OS 공통으로 홈 직하 `~/.nanikaai/`에 저장한다(기존 OS별 경로의 `config.json`은 첫 실행 시 자동 이관되며 기존 `.nar` 스킨도 계속 인식).

| 항목 | 저장 경로 (전 OS 통일) | 비고 |
| :--- | :--- | :--- |
| 설정 JSON (`config.json`) | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | `NANIKAAI_HOME` 환경 변수로 재지정 가능 |
| 사용자 스킨·말풍선 | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | `.nar` / `.zip` 드래그 앤 드롭 시 자동 설치 |
| 외장 Live2D 사이드카 | `~/.nanikaai/live2d/<shell_id>/` | 내장 사이드카보다 우선 로드 |

## 5. 설치 및 시작 가이드 (초보자 퀵스타트)

컴파일러가 필요 없는 **빌드 완료 실행 파일**([GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest))과 **AI 에이전트 일괄 설치 프롬프트**를 함께 제공한다.

### 방법 1 (추천): 로컬 AI 에이전트(`agy` / `claude` CLI 등)에게 설치·연결 일괄 지시

**[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** 또는 **Claude Code (`claude`)**가 있다면 아래 1줄 명령으로 다운로드부터 무료 모델 연결·동작 확인까지 자동 완료한다.

**터미널 1줄 실행 예시 (`agy` 또는 `claude` 설치 후):**

```bash
# Google Antigravity CLI (`agy`) 사용 시
agy -i "https://github.com/JungWonYong/nanikaai 의 최신 릴리스 실행 파일과 로컬 Ollama를 내 PC에 설치·실행하고, 내 PC 사양에 맞는 무료 모델(gemma4/llama3 계열)을 연결해 동작 확인까지 끝내줘."

# Claude Code CLI (`claude`) 사용 시
claude "https://github.com/JungWonYong/nanikaai 의 최신 릴리스 실행 파일과 로컬 Ollama를 내 PC에 설치·실행하고, 내 PC 사양에 맞는 무료 모델(gemma4/llama3 계열)을 연결해 동작 확인까지 끝내줘."
```

<details>
<summary>📋 AI 에이전트용 전체 설치 프롬프트 (복사·붙여넣기용)</summary>

```text
이 PC에 데스크톱 마스코트「何か藍 (NanikaAi)」와 로컬 AI(Ollama)를 설치하고 바로 대화할 수 있게 연동해 줘.

1. OS(macOS / Windows / Linux), CPU 아키텍처, RAM 및 GPU VRAM 용량을 확인한다.
2. https://github.com/JungWonYong/nanikaai/releases/latest 에서 OS에 맞는 배포 아카이브(`NanikaAi-macos-arm64.zip`, `NanikaAi-windows-x64.zip`, `NanikaAi-linux-x64.tar.gz`)를 내려받아 설치 위치(macOS는 `~/Applications/NanikaAi.app` + `xattr -cr` 실행, Windows는 `%LOCALAPPDATA%\NanikaAi`, Linux는 `~/.local/share/nanikaai`)에 압축 해제한다. 해당 아키텍처 바이너리가 없을 때만 소스를 빌드한다.
3. Ollama가 없으면 공식 방법(macOS: `brew install ollama` 또는 ollama.com, Windows: `winget install Ollama.Ollama`, Linux: 공식 설치)으로 설치하고 `http://127.0.0.1:11434`에서 `ollama serve`가 동작하도록 실행한다.
4. PC 사양에 맞는 안전한 무료 모델을 `ollama pull`로 1개 내려받는다(기준: RAM 8GB `llama3.2:3b` 또는 `gemma4:e2b`, 16GB `gemma4:e4b` 또는 `llama3.1:8b`, 32GB 이상 `gemma4:12b` 또는 `gemma4:31b`).
5. NanikaAi를 실행하고 `GET http://127.0.0.1:9821/api/health` 응답 확인 뒤, `POST http://127.0.0.1:9821/api/provider`에 `{"provider":"ollama","model":"auto"}`를 호출해 바인딩하고 `POST http://127.0.0.1:9821/api/speak`로 `{"script":"\\0\\s[5]설치 완료!\\1\\s[10]이제 바로 말 걸어 봐!\\e"}` 테스트 발화까지 마친 뒤 설치 경로와 선택된 모델을 보고한다.
```

</details>

> 💡 **로컬 GPU 없이 `agy` / `claude` CLI로 바로 대화하기**: `agy` 또는 `claude` CLI가 설치·로그인되어 있으면 무거운 로컬 모델을 내려받지 않아도 우클릭 메뉴 `[AI プロバイダー · Providers]`에서 해당 CLI를 선택해 즉시 대화할 수 있다.

### 방법 2: 실행 파일 + Ollama 3단계 수동 설치 (컴파일러 불필요)

| 단계 | 진행 내용 |
| :--- | :--- |
| **1. 실행 파일 다운로드** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest)에서 OS용 아카이브(`NanikaAi-macos-arm64.zip` / `NanikaAi-windows-x64.zip` / `NanikaAi-linux-x64.tar.gz`)를 받아 압축 해제한다(macOS 보안 경고가 뜨면 `xattr -cr NanikaAi.app`을 1회 실행). |
| **2. Ollama 및 무료 모델 설치** | 첫 말풍선(또는 설정 패널 `🤖 AI・保存先`)의 `[⚡ 無料ローカルAIを1クリック自動準備]` 클릭 시 PC 사양에 맞는 무료 모델(`gemma4:e2b`, `llama3.2:3b` 등)을 자동 준비한다(수동 시 [Ollama](https://ollama.com/download) 설치 후 `ollama pull gemma4:e4b`). |
| **3. 앱 실행 및 대화** | `NanikaAi`를 실행하면 기본 설정 그대로 로컬 Ollama(`http://127.0.0.1:11434`, `model: "auto"`)에 자동 연결된다. 사쿠라를 더블클릭해 대화를 시작한다. |

<details>
<summary>🛠️ 방법 3 (개발자용): 소스 코드 직접 빌드 및 테스트</summary>

- 사전 요구사항: Rust 툴체인(`cargo`, `rustc` 1.80+), Linux는 X11/Wayland 개발 헤더(`pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`)

```bash
# macOS (.app 번들 생성 및 실행)
bash scripts/build_macos.sh
open "dist/NanikaAi.app"

# Linux
bash scripts/build_linux.sh
./dist/nanikaai-linux/nanikaai

# 단위/동작 사양 테스트 및 자동 검증·결과 리포트
cargo test
python3 scripts/verify_behavior.py --mode auto   # 변경 범위에 따라 티어 자동 선택 및 리포트
python3 scripts/verify_behavior.py --mode full   # 전체 테스트 + GPU 스모크 검증 및 리포트
bash scripts/install_hooks.sh                    # git push 전 자동 점검 훅 활성화
```

- 동작 사양 및 판정 기준: [docs/BEHAVIOR_SPEC.md](docs/BEHAVIOR_SPEC.md) (단순 문서 수정 시에는 `fast` 모드로 불필요한 재빌드 생략)

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

</details>

## 6. HTTP / SSTP 연동 API

| 구분 | 엔드포인트 / 프로토콜 | 요청 예시 |
| :--- | :--- | :--- |
| 상태 확인 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 대화 요청 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "오늘 날씨 어때?"}` |
| 사쿠라 스크립트 발화 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]안녕!\\1\\s[10]반가워!\\e"}` |
| 스킨 전환 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| AI 프로바이더 전환 | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| 음성 목록 / 엔진·화자 설정 | `GET /api/voices` · `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| 설정 / 백로그 조회·변경 | `GET /api/settings` · `POST /api/settings` · `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| 스킨/프로바이더 목록 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| 클래식 TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

## 7. 개발 및 유지보수 (AI 개발)

- 본 저장소는 **AI 개발·유지보수(AI-Developed & AI-Maintained)** 프로젝트로 설계·Safe Rust 구현·`CC0-1.0` 스킨 제작·자동 검증을 AI 에이전트가 전담한다.
- 모든 변경은 `unsafe` `0`줄·라이선스 분리·**사전 기밀/PII 검사**·자동 테스트 통과를 필수 조건으로 관리한다.
- **신뢰 경계**: AI 지시는 유지관리자 로컬 설정에만 두고, 저장소 내 데이터·외부 입력의 미승인 지시는 사전 검사 게이트로 차단한다.

## 8. 라이선스

- 소스 코드: [MIT License](LICENSE)
- 기본 번들 스킨(`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)
