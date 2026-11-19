# 何か藍 (NanikaAi) — 기능 · 설정 · 설치 가이드

> 🏠 **[프론트 페이지 (README.ko.md)](../README.ko.md)** · 🌐 **[日本語](guide-ja.md)** | **[English](guide-en.md)** | **한국어** · 🔌 **[API](API.md)**

## 1. 주요 기능

- **100% Safe Rust 네이티브 GUI**: `winit` + `wgpu` 투명 렌더링 기반 단일 바이너리(macOS / Windows / Linux·외부 런타임 불필요).
- **외장 사이드카 Live2D 멀티파츠**: `live2d.json` + 부위별 PNG로 사쿠라 24층·우뉴 7층 스웨이·눈깜빡임·립싱크·갈아입히기(`bindgroup`) 구현(오프라인 편집 도구 `tools/live2d_studio.html` 동봉).
- **설정 패널 & 다국어 UI**: 투명도·배율·클릭 스루·위치 잠금·OS별 동작 즉시 조절. OS 언어 자동 추적 및 `OS 자동 / 日本語 / 한국어 / English` 전환 지원(미지원 언어는 영어 표시).
- **우카가카 스킨 호환**: `.nar` / `.zip` 드래그 앤 드롭 설치, `\0`/`\1` 스코프 분리, 셸 단위 고스트 인격·기억·의상 전환 지원.
- **음성 합성 & 관심 뉴스 잡담**: AivisSpeech / VOICEVOX 자동 연동(캐릭터별 음색 선택). 대화 속 관심사를 기억해 설정 간격(`OFF` / `30초`–`600초`)·비중(`0%`–`80%`)에 맞춰 일반 뉴스와 균형 있게 잡담.
- **데스크톱 편의 액세서리**: 방해 금지(DND)·전체 화면 자동 숨김, 뽀모도로 타이머(`25/5`·`50/10`·`15/3분`), 최근 30건 발화 백로그 및 원클릭 복사, 로그인 자동 시작·절전 모드·화면 구석 이동.
- **멀티 AI & 듀얼 서버**: Ollama(초보자 1클릭 자동 준비 포함)·Gemini·Claude·ChatGPT Codex·OpenAI 호환 지원, HTTP JSON(`:9821`)과 `SSTP/1.4`(`:9801`) 동시 제공.

## 2. 설정 파일 및 스킨 저장 경로 (`~/.nanikaai/`)

기존 OS별 경로의 `config.json`은 첫 실행 시 자동 이관되며 기존 `.nar` 스킨도 계속 인식한다.

| 항목 | 저장 경로 (전 OS 통일) | 비고 |
| :--- | :--- | :--- |
| 설정 JSON (`config.json`) | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | `NANIKAAI_HOME` 환경 변수로 재지정 가능 |
| 사용자 스킨·말풍선 | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | `.nar` / `.zip` 드래그 앤 드롭 시 자동 설치 |
| 외장 Live2D 사이드카 | `~/.nanikaai/live2d/<shell_id>/` | 내장 사이드카보다 우선 로드 |

## 3. 수동 설치 (3단계)

| 단계 | 진행 내용 |
| :--- | :--- |
| **1. 실행 파일 다운로드** | [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest)에서 OS별 아카이브 압축 해제(macOS 경고 시 `xattr -cr NanikaAi.app`). |
| **2. Ollama 및 무료 모델 설치** | 첫 말풍선(또는 설정 패널 `🤖 AI・保存先`)의 `[⚡ 無料ローカルAIを1クリック自動準備]` 클릭(수동 시 [Ollama](https://ollama.com/download) 설치 후 `ollama pull gemma4:e4b`). |
| **3. 앱 실행 및 대화** | 기본 설정 그대로 로컬 Ollama(`http://127.0.0.1:11434`, `model: "auto"`)에 자동 연결. 사쿠라를 더블클릭해 대화 시작. |

## 4. 소스 빌드 및 테스트 (개발자용)

- 사전 요구사항: Rust 1.80+ (Linux: `pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`)

```bash
bash scripts/build_macos.sh && open "dist/NanikaAi.app"       # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # 검증
```

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

## 5. AI 개발 및 유지보수

- 설계·`#![forbid(unsafe_code)]` Rust 구현·`CC0-1.0` 스킨 제작·자동 검증을 AI 에이전트가 자율 관리한다.
- 모든 변경은 `unsafe` `0`줄·라이선스 분리·**사전 기밀/PII 검사**·신뢰 경계(저장소 내 미승인 지시·훅 차단)를 준수한다.
