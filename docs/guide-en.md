# NanikaAi (何か藍) — Features, Config & Install Guide

> 🏠 **[Front Page (README.en.md)](../README.en.md)** · 🌐 **[日本語](guide-ja.md)** | **English** | **[한국어](guide-ko.md)** · 🔌 **[API](API.md)**

## 1. Key Features

- **100% Safe Rust Native GUI**: Single `winit` + `wgpu` transparent native binary for macOS, Windows, and Linux with zero external runtimes.
- **External-Sidecar Multi-Part Live2D**: Renders 24 Sakura and 7 Unyu anatomical layers (`live2d.json` + PNGs) for sway, blink, lip-sync, and `bindgroup` dress-up (includes offline editor `tools/live2d_studio.html`).
- **Settings Panel & Multilingual UI**: Adjust opacity, scale, click-through, position lock, and OS behavior live; tracks OS language automatically (`OS Auto / 日本語 / 한국어 / English`, English fallback on unsupported locales).
- **Ukagaka Skin Compatibility**: Drag-and-drop `.nar` / `.zip` installs with strict `\0`/`\1` isolation and per-shell Ghost persona, memory, and dress-up switching.
- **TTS Voice & Interest-Balanced News**: Headless AivisSpeech / VOICEVOX playback with voice casting, plus conversational interest memory mixed with general news at configurable intervals (`OFF` or `30s`–`600s`, `0%`–`80%` weight).
- **Desktop Companion Utilities**: Quiet / Do-Not-Disturb & fullscreen auto-hide, Pomodoro focus timer (`25/5`, `50/10`, `15/3m`), 30-entry dialogue backlog with 1-click copy, login auto-start, battery eco mode, and edge snap.
- **Multi-AI & Dual Server**: Connects Ollama (with 1-click beginner auto-setup), Gemini, Claude, ChatGPT Codex, and OpenAI-compatible endpoints while exposing HTTP JSON (`:9821`) and `SSTP/1.4` (`:9801`).

## 2. Configuration & User Skin Directories (`~/.nanikaai/`)

Legacy OS `config.json` files migrate automatically on first launch while legacy `.nar` skins remain discoverable.

| Purpose | Unified Home Path | Notes |
| :--- | :--- | :--- |
| Settings JSON | `~/.nanikaai/config.json` (Windows: `%USERPROFILE%\.nanikaai\config.json`) | Override root via `NANIKAAI_HOME` |
| User Skins (`.nar` / `.zip`) | `~/.nanikaai/shells/`, `~/.nanikaai/balloons/` | Auto-installed when dropped onto window |
| External Live2D Sidecars | `~/.nanikaai/live2d/<shell_id>/` | Overrides bundled sidecars cleanly |

## 3. Manual Install (3 Steps)

| Step | Instructions |
| :--- | :--- |
| **1. Download App Bundle** | Extract your OS package from [GitHub Releases (Latest)](https://github.com/JungWonYong/nanikaai/releases/latest) (on macOS, run `xattr -cr NanikaAi.app` if prompted). |
| **2. Install Ollama & Model** | Click `[⚡ 無料ローカルAIを1クリック自動準備]` in the welcome balloon (or Settings `🤖 AI・保存先`), or install [Ollama](https://ollama.com/download) and run `ollama pull gemma4:e4b`. |
| **3. Launch & Chat** | Default settings auto-connect to local Ollama (`http://127.0.0.1:11434`, `model: "auto"`). Double-click Sakura to chat. |

## 4. Build from Source & Test (Developers)

- Prerequisites: Rust 1.80+ (Linux: `pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev`)

```bash
bash scripts/build_macos.sh && open "dist/NanikaAi.app"       # macOS
bash scripts/build_linux.sh && ./dist/nanikaai-linux/nanikaai # Linux
cargo test && python3 scripts/verify_behavior.py --mode auto  # Verify
```

```bat
REM Windows
scripts\build_windows.bat
dist\NanikaAi-win64\nanikaai.exe
```

## 5. AI Development & Maintenance

- Architecture, `#![forbid(unsafe_code)]` Rust code, `CC0-1.0` skins, and automated verification are autonomously maintained by AI agents.
- Every change must pass zero `unsafe` checks, `MIT`/`CC0-1.0` separation, **pre-test PII/secret scanning**, and strict instruction-data trust boundary enforcement.
