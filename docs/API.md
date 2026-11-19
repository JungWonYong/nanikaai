# NanikaAi — HTTP / SSTP API Reference

> 🏠 **[README (JA)](../README.md)** · **[README (EN)](../README.en.md)** · **[README (KO)](../README.ko.md)** · 📋 **[BEHAVIOR_SPEC.md](BEHAVIOR_SPEC.md)**

NanikaAi exposes a local **HTTP JSON API** (`127.0.0.1:9821`) and a classic **SSTP/1.4 TCP server** (`127.0.0.1:9801`) simultaneously.

| Action | Endpoint / Protocol | Example Payload / Command |
| :--- | :--- | :--- |
| Health check | `GET http://127.0.0.1:9821/api/health` | `curl -s http://127.0.0.1:9821/api/health` |
| AI chat | `POST http://127.0.0.1:9821/api/chat` | `{"message": "Hello!"}` |
| Speak Sakura Script | `POST http://127.0.0.1:9821/api/speak` | `{"script": "\\0\\s[5]Hello!\\1\\s[10]Nice to meet you!\\e"}` |
| Switch skin | `POST http://127.0.0.1:9821/api/skin` | `{"shell": "neo_default", "balloon": "clean_default"}` |
| Switch AI provider | `POST http://127.0.0.1:9821/api/provider` | `{"provider": "ollama", "model": "auto"}` |
| Voices / TTS | `GET /api/voices` · `POST /api/voice` | `{"tts_engine": "auto", "kero_voice_archetype": "boy"}` |
| Settings / Backlog | `GET/POST /api/settings` · `GET /api/backlog` | `{"ui_language": "auto", "quiet_mode": false, "pomodoro_action": "start"}` |
| List skins / providers | `GET /api/skins` · `GET /api/providers` | `curl -s http://127.0.0.1:9821/api/providers` |
| Classic TCP `SSTP/1.4` | `127.0.0.1:9801` | `SEND SSTP/1.4` (`Charset: UTF-8`, `Script: ...`) |
