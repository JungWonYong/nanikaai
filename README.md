# AI Ukagaka (`v1.1.1`)

**100% Safe Rust (`#![forbid(unsafe_code)]`)** 기반 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어(Baseware)입니다.
macOS, Windows, Linux 환경에서 별도 런타임 설치나 OS 네이티브 핸들 `unsafe` 호출 없이 단일 네이티브 바이너리로 동작합니다.

## 주요 특징

1. **Zero `unsafe` 크로스플랫폼 네이티브 GUI & OS별 작업 영역 정밀 배치**
   - `winit 0.30` + `wgpu 24` 투명 알파 컴포지팅 + `cosmic-text 0.12` CJK 텍스트 렌더링
   - `display.scale` 기반 HiDPI(@2x) 캐릭터·말풍선 고해상도 렌더링 및 `Window::set_cursor_hittest` 클릭 투과(Click-through)
   - OS별 UI 인셋 자동 인식(macOS 메뉴바/Dock 크기·위치, Windows 작업 표시줄, X11/Wayland `_NET_WORKAREA` 및 패널 인셋) 및 발끝·단면 자동 접지(Ground snapping)
   - 전역 50ms 커서 폴링을 통한 **비활성 상태 머리 쓰다듬기(Wobble-tolerant Head Stroking)** 및 SERIKO 애니메이션 지원
   - 한국어/일본어/영어 IME 입력이 가능한 대화창(`Communicate`) 및 시스템 트레이(`tray-icon`) 통합
2. **엄격한 캐릭터 스코프 분리 (`\0` / `\1`)**
   - 사쿠라 스크립트 `\0`(주 캐릭터)와 `\1`(파트너 캐릭터)의 서피스 해결과 대화 히스토리를 완전 분리하여 스킨 혼선 방지
   - 기존 Ukagaka `.nar` / `.zip` 아카이브 드래그 앤 드롭 및 메뉴 설치 지원 (`.png` / `.pna` / Shift-JIS `surfaces.txt` 호환)
3. **클린 오픈소스 자산 분리 (`MIT` + `CC0-1.0`)**
   - 저장소 기본 탑재 스킨(`neo_default`, `clean_default`)은 본 프로젝트에서 직접 제작한 **CC0-1.0** 퍼블릭 도메인 원본 자산만 포함
     - **사쿠라(`neo_default` `\0`)**: 블루·화이트 컨셉을 계승한 성인 미래풍 전신 부츠 디자인 (`@2x` HiDPI, 발끝 포함)
     - **우뉴(`neo_default` `\1`)**: 아저씨 감성의 작은 둥근 선글라스를 코 아래쪽에 걸쳐 써서 위로 두 눈이 모두 보이는 파트너 + 가는 소닉 스타일 다리·대형 하이톱 운동화 (`@2x` HiDPI, 발끝 포함)
     - **모던 말풍선(`clean_default`)**: 반투명 펄 글래스모피즘 카드·그라데이션 상단 바·곡선형 말꼬리 및 스크롤바 (`@2x` HiDPI)
   - 개인 소장 클래식 셸/말풍선은 각 OS 사용자 데이터 디렉터리에서 자동 로드
4. **멀티 AI 백엔드 & SSTP/HTTP 듀얼 서버**
   - 로컬 Ollama (`gemma4` 등), 원격 Ollama, Gemini CLI/API, Claude CLI/API, ChatGPT Codex, OpenAI 호환 엔드포인트 지원
   - HTTP JSON (`127.0.0.1:9821`) 및 클래식 `SSTP/1.4` TCP (`127.0.0.1:9801`) 동시 제공

## 사용자 스킨(.nar) 저장 경로

외부 배포 `.nar` 또는 `.zip` 스킨을 앱 창에 드래그 앤 드롭하면 아래 경로에 자동 설치됩니다.

| 운영체제 | 사용자 데이터 디렉터리 |
| :--- | :--- |
| macOS | `~/Library/Application Support/AIUkagaka/` |
| Windows | `%APPDATA%\AIUkagaka\` |
| Linux | `$XDG_DATA_HOME/ai-ukagaka/` (기본 `~/.local/share/ai-ukagaka/`) |

## 빌드 및 실행 방법

### 공통 사전 요구사항
- Rust 툴체인 (`cargo`, `rustc` 1.80+)
- Linux 빌드 시 X11/Wayland/GTK 트레이 개발 헤더 (`libgtk-3-dev`, `libxdo-dev` 등 배포판 패키지)

### macOS (`.app` 번들 생성)

```bash
bash scripts/build_macos.sh
open "dist/AI Ukagaka.app"
```

### Linux

```bash
bash scripts/build_linux.sh
./dist/ai-ukagaka-linux/ai-ukagaka
```

### Windows

```bat
scripts\build_windows.bat
dist\AI-Ukagaka-win64\ai-ukagaka.exe
```

### 스모크 테스트 실행

```bash
cargo test
cargo run --release -- --smoke-test
```

## HTTP / SSTP 연동 API

- 상태 확인: `GET http://127.0.0.1:9821/api/health`
- AI 대화 요청: `POST http://127.0.0.1:9821/api/chat` (`{"message": "오늘 날씨 어때?"}`)
- 사쿠라 스크립트 발화: `POST http://127.0.0.1:9821/api/speak` (`{"script": "\\0\\s[5]안녕!\\1\\s[10]반가워!\\e"}`)
- 스킨 전환: `POST http://127.0.0.1:9821/api/skin` (`{"shell": "neo_default", "balloon": "clean_default"}`)
- 스킨/프로바이더 목록: `GET http://127.0.0.1:9821/api/skins`, `GET http://127.0.0.1:9821/api/providers`
- 클래식 TCP `SSTP/1.4`: `127.0.0.1:9801`

## 라이선스

- 소스 코드: [MIT License](LICENSE)
- 기본 번들 스킨(`assets/shells/neo_default`, `assets/balloons/clean_default`): CC0-1.0
