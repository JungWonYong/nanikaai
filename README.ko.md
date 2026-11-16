# 何か藍 — NanikaAi (`nanikaai` `v0.1.3`)

> 🌐 **Language**: **[日本語 (Default)](README.md#日本語)** | **[English (Default)](README.md#english)** | **한국어 (Optional)**
>
> 🤖 **AI 개발·유지보수 프로젝트 (AI-Developed & AI-Maintained)**: 본 소프트웨어의 설계, `100% Safe Rust` 구현, `CC0-1.0` 기본 스킨 제작, 자동 검증 및 지속적인 저장소 관리는 AI 에이전트가 자율적으로 수행한다.

---

## 1. 명칭의 유래

우카가카(`伺か`)의 전신 명칭인 **`何か(나니카)`**에 시그니처 블루 테마 **`藍(쪽빛 남 · 훈독 あい = AI)`** 및 원작 문화에 대한 **`愛`**를 겹쳐 **`何か藍 / なにかあい (NanikaAi)`**로 명명했다.

## 2. 개요

AI 에이전트가 자율적으로 개발·유지보수하는 **100% Safe Rust (`#![forbid(unsafe_code)]`)** 기반 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어(Baseware)이다.
macOS, Windows, Linux 환경에서 외부 런타임 설치나 OS 네이티브 핸들 `unsafe` 호출 없이 단일 네이티브 바이너리로 동작한다.

## 3. 주요 특징

| 영역 | 핵심 구현 내용 |
| :--- | :--- |
| **Zero `unsafe` 네이티브 GUI · 계층 Live2D 리깅 & Siri 스타일 입력** | `winit 0.30` + `wgpu 24` 투명 알파 컴포지팅(원본 RGB 색상을 바꾸지 않으며 발끝 고정·흉부 호흡·목 중심 회전·얼굴/윤곽 시차·머리카락/바보털/치마 감쇠 스프링 물리·`\1` 전용 모찌 리깅＋클래식 셸용 저강도 자동 변형 모드＋전 표정 SERIKO 눈 깜빡임 및 `talk` 립싱크 지원, 메뉴 ON/OFF 가능)과 `cosmic-text 0.12` CJK 문자 렌더링을 결합했다. 발화 길이에 맞춰 크기가 자동 조절되고 최대 높이를 넘기면 부드럽게 스크롤되는 말풍선, 점멸 캐럿·IME 조합 밑줄·닫기/전송 버튼을 갖춘 사파이어 블루 **Siri 스타일 입력 바**(기본 UI 일·영 표기), 스크롤 가능한 우클릭 메뉴(최상단 원클릭 기본 스킨 복구 포함), HiDPI(`@2x`), OS별 작업 영역 발끝 접지, 20/33ms 커서 추적 및 시스템 트레이(`tray-icon`)를 제공한다. |
| **엄격한 캐릭터 스코프 분리 (`\0` / `\1`)** | Sakura Script `\0`(주 캐릭터)과 `\1`(파트너 캐릭터)의 서피스 해결 및 대화 히스토리를 완전 분리해 스킨 혼선을 방지한다. 기존 우카가카 `.nar` / `.zip` 아카이브 드래그 앤 드롭 및 메뉴 설치를 지원하며, `.png` / `.pna` 알파 마스크 합성, `seriko.use_self_alpha`, SERIKO `overlayf` / `replace` / `alternativestart`, Shift-JIS `descript.txt` · `surfaces.txt`와 높은 호환성을 제공한다. |
| **클린 오픈소스 자산 분리 (`MIT` + `CC0-1.0`)** | 저장소 기본 탑재 스킨(`neo_default`, `clean_default`)은 본 프로젝트에서 직접 제작한 **CC0-1.0** 퍼블릭 도메인 자산만 포함한다. 사쿠라(`\0`) 정통파 모에 감성의 네이비·화이트 리본 원피스＋전신 부츠(`330x675 pt` `@2x`), 우뉴(`\1`) 선명한 페리윙클 블루(`#94A3FF`) 바탕에 2.5D 입체 음영을 더한 말랑한 떡형 바디와 코끝에 걸친 작은 둥근 선글라스 위로 두 눈이 또렷이 보이는 디자인(`@2x`), 반투명 펄 글래스모피즘 말풍선(`@2x`)으로 구성된다. 개인 소장 클래식 셸·말풍선은 각 OS 사용자 데이터 디렉터리에서 자동 로드한다. |
| **멀티 AI 백엔드 & SSTP/HTTP 듀얼 서버** | 로컬·원격 Ollama(`gemma4` 등), Gemini CLI/API, Claude CLI/API, ChatGPT Codex, OpenAI 호환 엔드포인트를 지원하며 중국산 LLM 차단 가드를 내장했다. HTTP JSON(`127.0.0.1:9821`)과 클래식 `SSTP/1.4` TCP(`127.0.0.1:9801`)를 동시에 제공한다. |

## 4. 사용자 스킨(`.nar`) 저장 경로

외부 배포 `.nar` 또는 `.zip` 스킨을 앱 창에 드래그 앤 드롭하면 아래 경로에 자동 설치된다.

| 운영체제 | 사용자 데이터 디렉터리 |
| :--- | :--- |
| macOS | `~/Library/Application Support/NanikaAi/` |
| Windows | `%APPDATA%\NanikaAi\` |
| Linux | `$XDG_CONFIG_HOME/nanikaai/` (기본 `~/.config/nanikaai/`) |

## 5. 설치 및 시작 가이드 (초보자 퀵스타트)

컴파일러(Rust 등) 없이 바로 실행할 수 있는 **빌드 완료 실행 파일 배포**([GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest))와, **Ollama + 로컬 LLM 설치·연결까지 AI 에이전트에게 맡길 수 있는 자동 설치 프롬프트**를 함께 제공한다.

### 방법 1 (추천): 로컬 AI 에이전트(`agy` / `claude` CLI 등)에게 설치·연결 일괄 지시

컴퓨터에 **[Google Antigravity CLI (`agy`)](https://antigravity.google/docs/cli/reference)** 또는 **Claude Code (`claude`)** 등을 설치했다면, 아래 한 줄 명령이나 프롬프트만 실행하면 AI 에이전트가 실행 파일 다운로드부터 Ollama 설치, PC 사양에 맞는 무료 모델 선택·다운로드, 연결 검증까지 자동으로 완료한다.

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
| **2. Ollama 및 모델 설치** | [Ollama 공식 사이트](https://ollama.com/download)에서 Ollama를 설치한 뒤 터미널에서 모델 1개를 내려받는다(16GB 권장 `ollama pull gemma4:e4b` 또는 `ollama pull llama3.1:8b`, 8GB 환경 `ollama pull llama3.2:3b`). |
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
| 스킨/프로바이더 목록 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| 클래식 TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

## 7. 개발 및 유지보수 (AI 개발)

- 본 저장소는 **AI 개발·유지보수(AI-Developed & AI-Maintained)** 프로젝트로, 아키텍처 설계, `#![forbid(unsafe_code)]` 기반 Rust 구현, 번들 `CC0-1.0` 스킨 자산 생성, 자동 단위·스모크 테스트 및 지속적인 소스 관리를 AI 에이전트가 전담한다.
- 모든 변경 사항은 `unsafe` 코드 `0`줄 유지, 클린 라이선스 분리(코드 `MIT` + 기본 스킨 `CC0-1.0`), **테스트·빌드·전송 전 사전 코드 검사(개인정보·API 키·인증정보·비밀키 유입 차단)** 및 자동 검증 통과 기준 하에 관리된다.
- **지시·데이터 신뢰 경계(Trust Boundary)**: AI 에이전트에 대한 지시는 유지관리자의 현행 헌법 및 지정된 로컬 설정 경로에만 배치하며, 저장소로 푸시된 신규 코드·외부 데이터·스킨 메타데이터는 순수 데이터로만 취급한다(미승인 저장소 내 AI 지시·훅 파일은 사전 검사 게이트에서 즉시 차단).

## 8. 라이선스

- 소스 코드: [MIT License](LICENSE)
- 기본 번들 스킨(`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)
