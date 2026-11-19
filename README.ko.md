# 何か藍 — NanikaAi (`v0.1.10`)

> 🌐 **Language**: **[日本語 (Default)](README.md#日本語)** | **[English (Default)](README.md#english)** | **한국어 (Optional)** · 🎬 **[데모 페이지](https://jungwonyong.github.io/nanikaai/?lang=ko)** · 📖 **[상세 가이드 (`docs/GUIDE.md`)](docs/GUIDE.md#한국어)**

---

**100% Safe Rust (`#![forbid(unsafe_code)]`)** 기반으로 AI 에이전트가 자율 개발·유지보수하는 macOS / Windows / Linux 크로스플랫폼 AI 데스크톱 마스코트 베이스웨어이다.
우카가카(`伺か`)의 전신 명칭 **「何か」**에 시그니처 블루 **「藍(AI / 愛)」**을 결합해 명명했다.

- 📥 **[최신 릴리스 (GitHub Releases)](https://github.com/JungWonYong/nanikaai/releases/latest)** · 📖 **[상세 가이드 (`docs/GUIDE.md`)](docs/GUIDE.md#한국어)** · 📋 **[동작 사양 (`docs/BEHAVIOR_SPEC.md`)](docs/BEHAVIOR_SPEC.md)**

## 🎬 소개 동영상 (한국어판)

https://github.com/user-attachments/assets/a9e7e117-35c1-4a71-a4b2-c1e5234186ba

- 🌐 **[다국어 전환 데모 페이지 (GitHub Pages)](https://jungwonyong.github.io/nanikaai/?lang=ko)** · 📥 MP4: **[한국어](docs/media/nanikaai_intro_ko.mp4)** / **[日本語](docs/media/nanikaai_intro_ja.mp4)** / **[English](docs/media/nanikaai_intro_en.mp4)** (우측 하단 스피커로 음소거 해제)

## 🚀 빠른 시작

- **실행 파일**: [GitHub Releases](https://github.com/JungWonYong/nanikaai/releases/latest)에서 압축 해제 후 첫 말풍선의 `[⚡ 無料ローカルAIを1クリック自動準備]` 클릭만으로 바로 시작할 수 있다.
- **AI 에이전트 1줄 자동 설치 (`agy` / `claude`)**:

```bash
agy -i "https://github.com/JungWonYong/nanikaai 의 최신 릴리스 실행 파일과 로컬 Ollama를 내 PC에 설치·실행하고, 내 PC 사양에 맞는 무료 모델(gemma4/llama3 계열)을 연결해 동작 확인까지 끝내줘."
```

<details>
<summary>✨ 주요 기능 요약 (클릭하여 펼치기)</summary>

- **100% Safe Rust 네이티브 GUI**: `winit` + `wgpu` 투명 렌더링 기반 단일 바이너리(macOS / Windows / Linux·외부 런타임 불필요).
- **외장 사이드카 Live2D 멀티파츠**: `live2d.json` + 부위별 PNG로 사쿠라 24층·우뉴 7층 스웨이·눈깜빡임·립싱크·갈아입히기(`bindgroup`) 구현(오프라인 편집 도구 `tools/live2d_studio.html` 동봉).
- **설정 패널 & 다국어 UI**: 투명도·배율·클릭 스루·위치 잠금·OS별 동작 즉시 조절. OS 언어 자동 추적 및 `OS 자동 / 日本語 / 한국어 / English` 전환 지원(미지원 언어는 영어 표시).
- **우카가카 스킨 호환**: `.nar` / `.zip` 드래그 앤 드롭 설치, `\0`/`\1` 스코프 분리, 셸 단위 고스트 인격·기억·의상 전환 지원.
- **음성 합성 & 관심 뉴스 잡담**: AivisSpeech / VOICEVOX 자동 연동(캐릭터별 음색 선택). 대화 속 관심사를 기억해 설정 간격(`OFF` / `30초`–`600초`)·비중(`0%`–`80%`)에 맞춰 일반 뉴스와 균형 있게 잡담.
- **데스크톱 편의 액세서리**: 방해 금지(DND)·전체 화면 자동 숨김, 뽀모도로 타이머(`25/5`·`50/10`·`15/3분`), 최근 30건 발화 백로그 및 원클릭 복사, 로그인 자동 시작·절전 모드·화면 구석 이동.
- **멀티 AI & 듀얼 서버**: Ollama·Gemini·Claude·ChatGPT Codex·OpenAI 호환 지원, HTTP JSON(`:9821`)과 `SSTP/1.4`(`:9801`) 동시 제공.
- **라이선스 분리**: 소스 코드 `MIT` + 자체 제작 기본 스킨(`neo_default` / `clean_default`) `CC0-1.0`.

</details>

<details>
<summary>⚙️ 설정 경로 (~/.nanikaai/) · 수동 설치 · 소스 빌드</summary>

- **통일 저장 경로**: 설정 `~/.nanikaai/config.json`, 스킨 `~/.nanikaai/shells/`·`~/.nanikaai/balloons/`, 외장 Live2D `~/.nanikaai/live2d/<shell_id>/` (기존 OS별 설정 자동 이관).
- **3단계 수동 설치 및 소스 빌드 안내**: **[상세 가이드 (`docs/GUIDE.md`)](docs/GUIDE.md#한국어)** 참조.

</details>

<details>
<summary>🔌 HTTP / SSTP API · AI 개발/유지보수 · 라이선스</summary>

- **API**: HTTP JSON(`127.0.0.1:9821`: `/api/health`, `/api/chat`, `/api/speak`, `/api/skin`, `/api/provider`, `/api/voice`, `/api/settings`, `/api/backlog`) 및 `SSTP/1.4`(`127.0.0.1:9801`). 상세 예시는 **[docs/GUIDE.md](docs/GUIDE.md#한국어)**.
- **AI 개발·유지보수**: 설계·Safe Rust 구현·`CC0-1.0` 스킨·사전 기밀/PII 검사·자동 테스트를 AI 에이전트가 자율 관리(미승인 저장소 내 지시·훅 자동 차단).
- **라이선스**: 소스 코드 [MIT License](LICENSE) / 기본 스킨(`CC0-1.0`) [neo_default](assets/shells/neo_default/LICENSE-CC0.txt), [clean_default](assets/balloons/clean_default/LICENSE-CC0.txt).

</details>
