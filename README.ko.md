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

## 5. 빌드 및 실행 방법

### 공통 사전 요구사항

- Rust 툴체인 (`cargo`, `rustc` 1.80+)
- Linux 빌드 시 X11/Wayland/GTK 트레이 개발 헤더 (`libgtk-3-dev`, `libxdo-dev` 등 배포판 패키지)

### OS별 빌드 및 실행

```bash
# macOS (.app 번들 생성 및 실행)
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

### 테스트 및 스모크 검증

```bash
cargo test
cargo run --release -- --smoke-test
```

## 6. HTTP / SSTP 연동 API

| 구분 | 엔드포인트 / 프로토콜 | 요청 예시 |
| :--- | :--- | :--- |
| 상태 확인 | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI 대화 요청 | `POST http://127.0.0.1:9821/api/chat` | `{"message": "오늘 날씨 어때?"}` |
| 사쿠라 스크립트 발화 | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]안녕!\\1\\s[10]반가워!\\e"}` |
| 스킨 전환 | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| 스킨/프로바이더 목록 | `GET http://127.0.0.1:9821/api/skins` / `/api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| 클래식 TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |

## 7. 개발 및 유지보수 (AI 개발)

- 본 저장소는 **AI 개발·유지보수(AI-Developed & AI-Maintained)** 프로젝트로, 아키텍처 설계, `#![forbid(unsafe_code)]` 기반 Rust 구현, 번들 `CC0-1.0` 스킨 자산 생성, 자동 단위·스모크 테스트 및 지속적인 소스 관리를 AI 에이전트가 전담한다.
- 모든 변경 사항은 `unsafe` 코드 `0`줄 유지, 클린 라이선스 분리(코드 `MIT` + 기본 스킨 `CC0-1.0`), 자동 검증 통과 기준 하에 관리된다.

## 8. 라이선스

- 소스 코드: [MIT License](LICENSE)
- 기본 번들 스킨(`CC0-1.0`): [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt)
