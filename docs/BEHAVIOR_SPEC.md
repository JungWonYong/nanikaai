# 何か藍 (`NanikaAi`) — 現行動作仕様および自動検証基準 (`BEHAVIOR_SPEC`)

- **Document Version**: `v1.18.0`
- **Target Software**: `nanikaai` `v0.1.11+`
- **Purpose**: テスト実行前の機密・個人情報・AI指示配置境界検査（Phase 0）、改修後の自動動作確認テスト（`cargo test` / `--smoke-test` / 静的監査）における合否基準、変更内容に応じた検証ティアおよび結果報告様式を定義する。

---

## 1. テスト前事前検査（Phase 0）および変更規模別の自動検証ティア規約

すべての自動検証・SSH リモート転送・Git コミット/プッシュに先立ち、**Phase 0 事前コード検査** を最優先で実行し、不合格時はビルドやテストを開始せず即時停止する。Phase 0 合格後、修正内容に応じて検証範囲を自動選択する（`python3 scripts/verify_behavior.py --mode auto`）。

| 段階 / ティア | 適用条件（自動判定基準） | 実施内容 |
| :--- | :--- | :--- |
| **Phase 0: `pretest`** | すべての検証・コミット（`pre-commit`）・プッシュ（`pre-push`）・CI 実行の**最前段（必須）** | テスト・ビルド・SSH 転送を行う前に、個人情報（ローカルユーザー名・個人ホームパス・非公開 LAN IP・個人メール）、API キー・認証情報・`.env` / `credentials*.json`、秘密鍵、および未承認のリポジトリ内 AI 指示/フックファイルを走査・遮断する。 |
| **Tier 0: `fast`** | 文書のみの変更（`*.md`・`LICENSE`・`.gitignore` のみ、ただし `assets/` 配下を除く） | Phase 0 通過後、再ビルド不要の高速静的検査（`#![forbid(unsafe_code)]` 維持、ライセンス分離確認、残留マーカー `0` 件監査）を実施する。 |
| **Tier 1: `standard`** | ロジック・サーバー・AI・パーサー・テスト・設定・CI 定義の変更 | Phase 0 ＋ Tier 0 ＋ 単体テスト全件および [`behavior_spec.rs`](../tests/behavior_spec.rs) 統合仕様テスト一式（`cargo test`）を実施する。 |
| **Tier 2: `full`** | GUI (`src/gui.rs`)・シェル描画 (`src/shell_parser.rs`)・ビルドスクリプト・`assets/` の変更（非 CI のデスクトップ環境）、またはリリース前手動実行 | Tier 1 ＋ 必要時リリースビルド ＋ 実ウィンドウ/GPU スモーク検証（`--smoke-test`）＋ 稼働中ローカル API ヘルス確認を実施する。 |

---

## 2. 現行動作仕様および判定基準一覧（46 項目）

| 仕様 ID | 領域 | 期待される現行動作（仕様） | 合格基準（パス条件） | 検証手段 |
| :--- | :--- | :--- | :--- | :--- |
| `SPEC-SAFE-01` | 安全性 | 100% Safe Rust を厳守する | `src/lib.rs` と `src/main.rs` 先頭に `#![forbid(unsafe_code)]` が存在し、`src/` に `unsafe` 構文が `0` 件であること | `verify_behavior.py` / `behavior_spec` |
| `SPEC-SAFE-02` | 権利分離 | プログラムは `MIT`、同梱既定スキン・Live2D パーツ資産は `CC0-1.0` に分離する | `LICENSE`、`neo_default/LICENSE-CC0.txt`、`live2d/neo_default/LICENSE-CC0.txt`、`clean_default/LICENSE-CC0.txt`、`tools/live2d_studio.html`、`docs/BEHAVIOR_SPEC.md` が存在し非空であること | `verify_behavior.py` / `behavior_spec` |
| `SPEC-SAFE-03` | ツール保護 | 機密ファイル参照と SSRF（非公開 IP・ローカル帯域アクセス）を遮断する | `is_sensitive_path` が `.ssh`/`.aws`/`.nanikaai/config.json`/`credentials*.json`/`.env` を拒否し、`validate_public_url` が `127.0.0.1`/`169.254.169.254`/プライベート IP(`10.x`/`172.16.x` 等)/`file://` を拒否すること | `behavior_spec` / unit tests |
| `SPEC-SKIN-01` | 既定優先順位 | 同梱既定スキンを一覧の先頭に固定し、いつでも復帰可能とする | `ShellManager::list_shells()[0].id == "neo_default"` かつ `BalloonManager::list_balloons()[0].id == "clean_default"` であること | `behavior_spec` / `--smoke-test` |
| `SPEC-SKIN-02` | スコープ分離 | `\0`（メイン）と `\1`（パートナー）のサーフェス解決と表示を分離する | `resolve_surface_id(Scope::Kero, 5) >= 10` を満たし、`neo_default` の `\0`(surface `0`) と `\1`(surface `10`) が異なる非空 RGBA 画像であること | `behavior_spec` / `--smoke-test` |
| `SPEC-SKIN-03` | HiDPI・SERIKO | `neo_default` を `@2x` 解像度と目パチ/発話アニメーション付きで読み込む | `descript.display_scale` が `2.0` であり、surface `0` と `10` の双方に `talk`/`always` 等の SERIKO アニメーションが定義されること | `behavior_spec` / unit tests |
| `SPEC-SKIN-04` | クラシック互換 | `.pna` アルファマスク透過と `.nar` パストラバーサル防御が機能する | `.pna` で `(0,0,0)` 黒衣装が不透明保持・背景が `alpha == 0` になり、`../../` を含む `.nar` 導入で親ディレクトリ外へ書込まないこと | `behavior_spec` / `--smoke-test` |
| `SPEC-DLG-01` | スクリプト解析 | 話者切替・表情・選択肢・一時停止・早期 `\e` 回復を正確に解釈する | `\0` / `\1` / `\s[n]` / `\q[label,id]` / `\wN` / `\e` が対応する `ActionKind` へ変換され、早期 `\e` 後の発話も失われないこと | `behavior_spec` / unit tests |
| `SPEC-DLG-02` | タグ・文字浄化 | LLM 思考タグ・バイトトークン・HTML 残骸・未対応制御タグ・感情ト書き括弧を発話へ出さない | `<\|thought\|>...<\|/thought\|>`、`<end_of_turn>`、`<0xE3><0x80><0x80>` が `U+3000` 復元または完全除去され、`\0\s[1](困り/照れ)` 等の行頭/タグ直後感情ラベルを表情解決のうえ除去し本文中の `(macOS)`・`（笑）` を保持すること | `behavior_spec` / unit tests |
| `SPEC-DLG-03` | 履歴保護 | 会話履歴保存時に `\1` スコープ不整合と制御タグを再帰浄化する | `sanitize_reply_for_history` が `\1\s[5]` を `\1` 側サーフェス(`12`)へ補正し `<|thought|>` 等を残さないこと | `behavior_spec` / unit tests |
| `SPEC-AI-01` | モデル規制 | 制限対象 LLM（`FORBIDDEN_MODEL_PREFIXES`）の呼び出しを拒否する | 禁止プレフィックス全種で `assert_allowed_model` が `Err` を返し、`PREFERRED_MODELS` に禁止モデルが含まれないこと | `behavior_spec` / unit tests |
| `SPEC-AI-02` | Ollama 適応 | 初心者向けローカルモデル候補を幅広に自動選択する | `PREFERRED_MODELS` に `gemma4:e4b`・`llama3.2:3b` 等が含まれ、`provider_catalog()` の `"ollama"` 既定モデルが `"auto"` であること | `behavior_spec` / unit tests |
| `SPEC-AI-03` | 設定永続化 | `config.json` と記憶データをディスクへ保存・再読込できる | `ConfigStore::update(ConfigPatch)` 後の値が新規 `ConfigStore::new` インスタンスで一致すること | `behavior_spec` / unit tests |
| `SPEC-AI-04` | 無料ローカルAI 1クリック自動準備＆一般向け既定順序 | 新規ユーザー向けに `"ollama"` を `provider_catalog()` 先頭に配置し（`"linux-ollama"` は互換維持で末尾）、起動/オフライン時吹き出し・設定パネル Tab 3・メニューの 1 クリック操作で PC メモリ容量に応じた安全な無料軽量モデル（`gemma4:e2b` / `gemma4:e4b` / `llama3.2:3b`）を自動取得・接続できること | `provider_catalog()[0].id == "ollama"`、RAM 別の `recommend_local_ollama_candidates` 全候補が `assert_allowed_model` を通過、模擬 `/api/pull` 進捗ストリーム完了まで `prepare_local_ollama_blocking` が成功し、オフライン応答が `__setup_local_ai__` 選択肢を含むこと | `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-UI-01` | GPU/Live2D 基盤 | シェーダ定数レイアウトと 2D sRGB 前乗算合成・減衰バネ物理・足元固定表示が破綻しない | `ShaderParams` が `96 B`（WGSL 整合）であり、`Rgba8Unorm` 2D sRGB 空間で合成・前乗算出力し、スモーク検証で `neo_default` の足元・短文吹き出し・スクロール長文・メニューを正常描画すること | unit tests / `--smoke-test` |
| `SPEC-UI-02` | クロス OS 構成 | macOS / Windows ではトレイ統合し、Linux は右クリックメニューで完結する | `tray_icon` 依存が `macos`/`windows` のみに限定され、Linux/Windows/macOS 全ターゲットでコンパイル可能であること | CI / `cargo test` |
| `SPEC-UI-03` | リリース版数 | `Cargo.toml` と配布バンドル版数が一致し、文書過負荷更新を避ける | `Cargo.toml` の版数と `scripts/build_macos.sh` のバンドル版数が一致し、`scripts/build_macos.sh` が正常終了すること | `verify_behavior.py --mode full` |
| `SPEC-UI-04` | GUI 対話操作性 | 右クリックメニュー展開/ホバー/スクロール/切替実行、キャラダブルクリック入力バー展開、IME入力/編集/送信(`↑`)/`×`/`Esc`、吹き出し早送り/選択肢ホバー/スクロール/クリック非表示が正常に動作する | `--smoke-test` Phase E において全ウィンドウ対話操作・ヒット判定・状態遷移が 1 件も欠落せず合格すること | `--smoke-test` Phase E (`SPEC-UI-04`) |
| `SPEC-UI-05` | 3秒以内反応性 SLA | ユーザー操作（送信・選択肢・右クリック・入力バー・制御API・オフライン縮退）から 3 秒（`3,000 ms`）以内に視覚反応または応答を返す | 送信/選択直後の「AI 応答準備中…」即時表示（`< 500 ms` ステータス通知）、GUI 全操作合計 `ui_ms <= 3,000 ms`、HTTP/TCP 制御応答 `ipc_ms <= 3,000 ms`、オフライン即断 `< 3,000 ms` を実測で満たすこと | `--smoke-test` Phase E / `behavior_spec` |
| `SPEC-SRV-01` | HTTP 状態 API | `GET /api/health`・`/api/skins`・`/api/providers`・`/api/voices`・`/api/settings`・`/api/backlog`・`/api/memos`・`/api/calendar` が JSON (`ok: true`) を返す | 一時的 `start_dual_server(0, 0)` に対して全エンドポイントが `200 OK` かつ `"ok": true` を返し、`/api/settings`・`/api/calendar` に `api_key` や秘密 ICS URL を含まないこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-02` | HTTP 操作 API | `POST /api/speak`・`/api/chat`・`/api/event`・`/api/skin`・`/api/provider`・`/api/voice`・`/api/settings`・`/api/memos`・`/api/calendar` を処理する | 正常ペイロードで `200 OK`、必須項目欠落や不正値（空 `script`/`message`/`provider`、不正 `tts_engine`/`install_aivm_uuid`/`uninstall_aivm_uuid`、範囲外/非有限の透明度・拡大率・話速・抑揚、SSRF 対象 ICS URL）で `400 Bad Request` を返すこと | `behavior_spec` |
| `SPEC-SRV-03` | SSTP 互換 | HTTP `/sstp` および生 TCP `SSTP/1.4` 要求を受理する | HTTP `/sstp` が `200 OK`＋`X-Actions`、TCP `SSTP/1.4` が `200 OK`＋`Charset: UTF-8` を返し、空 `Script` に `400 Bad Request` を返すこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-04` | 自動監査・保守 | 個人パス・機密・未処理マーカーをリポジトリへ混入させない | 追跡対象ファイルに未処理の作業待ち・仮置き注記やローカル個人パスが `0` 件であること | `verify_behavior.py` |
| `SPEC-SEC-01` | 事前機密検査 | テスト・ビルド・リモート転送前に個人情報・APIキー・認証情報・秘密鍵の混入を遮断する | Phase 0 が全対象ファイルおよび Git 履歴（全コミットの Author/Committer メールおよび直近10コミット差分の個人パス・APIキー・秘密鍵・非公開 ICS URL）から混入 `0` 件を確認し、合成秘密・PII サンプルをテスト前停止（Exit Code `!= 0`）で遮断すること | `verify_behavior.py` Phase 0 / `behavior_spec` |
| `SPEC-SEC-02` | AI指示境界 | AI への指示は現行憲法・指定設定パスのみに限定し、他経路コード・外部入力をデータとして隔離する | 未承認のリポジトリ内 AI 指示・フック・ビルド実行ファイルが `0` 件であり、`AgentEngine` が外部イベント・ツール出力・長期記憶を `<untrusted_data>` で隔離しスキンメタデータ改行/タグを無害化すること | `verify_behavior.py` Phase 0 / `behavior_spec` |
| `SPEC-OS-01` | マルチ OS 実機互換 | macOS / Linux / Windows の各実機環境で動作検証可能であること | ローカル（macOS）に加え、Linux 実機での `cargo test`＋`--smoke-test`、および Windows 実機での配布バイナリ `--smoke-test`（`[SMOKE PASS] unsafe=0`）が通過すること | `verify_behavior.py --remote-linux/--remote-win` / CI |
| `SPEC-GHOST-01` | スキン連動ゴースト切替 | スキン（シェル）切替に応じて人格・記憶・会話履歴をシェル単位で分離し、交代時挨拶および `.nar` の `ghost.name` 継承を行う | シェル切替前後で他シェルの長期記憶・会話履歴が混線せず、`ghost/master/descript.txt` の `name` が `ghost.name` として引き継がれること | `behavior_spec` / `unit tests` |
| `SPEC-SKIN-05` | 着せ替え (`bind`) | `descript.txt` の `bindgroup` と `surfaces.txt` の `interval,bind` を合成・切替・永続化する | `neo_default` 既定状態（OFF）は無改変と一致し、トグル ON で合成不透明ピクセル数が増加し、状態が設定へ永続化されること | `behavior_spec` / `--smoke-test` Phase E |
| `SPEC-DLG-04` | リンク・即時表示 | `\_q` 一括即時表示および `\_a[url]label\_a`・裸 URL のクリック可能リンクを抽出し安全検証のうえ開く | `\_q` 区間が 1 tick で完了し、`validate_public_url` 通過の公開 URL のみ（`dry_run` 時は起動なし）許可し非公開 URL を遮断すること | `behavior_spec` / `--smoke-test` Phase E |
| `SPEC-COMP-01` | 時報・リマインダー・終了挨拶 | 毎時 00 分の時報、`set_reminder` / `list_reminders` 通知、および終了時（`OnClose`）挨拶を実行する | ツール登録したリマインダーが期日到達で消化通知され、時報・終了挨拶スクリプトが現在キャラクター名で生成されること | `behavior_spec` / `agent_engine` |
| `SPEC-COMP-02` | ファイル D&D 反応 | `.nar`/`.zip` 以外の汎用ファイル（文章・コード・画像・URL ショートカット・フォルダ）のドロップ内容を受理し要約反応する | `.env`/秘密鍵/資格情報ファイルを読込前に即拒否し、安全ファイル内容のみ `<untrusted_data>` で囲んで抽出すること | `behavior_spec` / `companion_extras` |
| `SPEC-COMP-03` | 音声読み上げ・更新確認 | 既定 OFF の AivisSpeech (`:10101`) / VOICEVOX (`:50021`)＋OS TTS フォールバックおよび GitHub Releases 更新確認を提供する | `\0`/`\1` バッチ発話抽出および SemVer 版数比較が正確に機能し、スモーク検証時は外部起動・音声ブロックを発生させないこと | `behavior_spec` / `companion_extras` |
| `SPEC-COMP-04` | ヘッドレス音声・`\0`/`\1` 別声自動配役＆エンジン選択 | 導入済み `AivisSpeech Engine` / `VOICEVOX vv-engine` / OS 標準音声を切替・自動起動し、シェルの `\1` 属性（少年系・女性相棒・青年男性）に応じて `\0`/`\1` を別声優・別モデル（AivisHub `らせつん` 自動/手動導入含む）へ自動配役する | `/speakers` 解析、`infer_kero_voice_archetype` / `pick_speaker_pair_for_archetype` による `\0`/`\1` 別声選出、および `tts_engine`・`kero_voice_archetype`・`sakura_speaker_id`/`kero_speaker_id` の永続化が一致すること | `behavior_spec` / `companion_extras` |
| `SPEC-COMP-05` | 雑談頻度(`30秒`〜)＆会話関心記憶・一般/関心バランス最新ニュース雑談 | 設定パネル・メニュー・`/api/settings` で自動雑談間隔を `OFF` または最短 `30秒`-`1800秒`（UI `30秒`-`600秒`）・関心ニュース比重を `0%`-`80%`（既定 `50%`）に調整でき、会話から抽出した関心事をシェル別に永続化したうえで、一般ニュース5分野と関心最新ニュース RSS を `<untrusted_data>` 隔離で偏りなく出題すること | `clamp_auto_talk_interval_sec(15)==30`、`POST /api/settings` の `30`/`0.75` 受理と範囲外 `400` 拒否、`extract_interests_from_utterance`（否定除外含む）＋`interest_save`＋シェル分離、比重 `0%`/`50%`/`80%` の配分比率（`80%` でも一般枠を残存）、RSS 解析・重複抑制・Phase E7 スライダー動作が通過すること | `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-L2D-01` | 外付けサイドカー Live2D マルチパーツ | 既存 `.nar` を無改変のまま外付け `live2d.json`＋パーツ PNG 階層を読み込み、ユーザー配置優先で解剖学的関節（首/C1 頭部/顔視差/多層髪/衣装裾・閉輪郭リボン/腕）・`surface_overrides` 無欠損表情・`AlphaMask` 合成・層別 `opacity`（40%半透明サングラス）・階層ボーン/減衰バネを CPU/GPU で駆動する | `neo_default` で `\0`(24層以上: 解剖学的首/C1/髪/衣装/フリル靴下＆メリージェーン靴・閉じ目 `[350..390, 260..300]` で `sakura_hair_curl_r.png` 透過保持・胸リボン`(309,508)`/`ボタン(328,668)` `alpha=255`・静止衣装差分 `<250 px`・全身静止差分 `<200 px`)と `\1`(6層以上: 無欠損 `surface11` 目元・alpha 40%レンズ)が読込まれ、表情/着せ替え差分反映・物理変位描画および `<user_data>/live2d` 優先上書きが通過すること | `behavior_spec` / `--smoke-test` |
| `SPEC-L2D-02` | サイドカー信頼境界＆オフラインスタジオ | `live2d.json` を純粋データとして検証し、パストラバーサル・機密参照・循環ボーンを遮断するとともにオフライン専用編集 UI を同梱する | `../`・`.env`・絶対パス・循環親参照が拒否され、`tools/live2d_studio.html` に外部 CDN URL が `0` 件であること | `behavior_spec` / `live2d_rig` |
| `SPEC-CFG-01` | ホーム直下設定永続化・旧環境自動移行 | 全 OS でホーム直下の `~/.nanikaai/config.json`（`NANIKAAI_HOME` 上書き可）へ JSON 保存し、旧 OS データ領域の既存 `config.json` を無損失で自動移行したうえで旧配下の `.nar` スキン参照も継続し、`is_sensitive_path` で機密保護する | `default_user_data_dir()` が `.nanikaai` で終わり、`ConfigStore::new_with_migration` が旧 `config.json` 移行時に範囲クランプ（`skin_opacity` `0.15-1.0` / `balloon_opacity` `0.25-1.0` / `character_scale` `0.50-1.50`）および初期化復元を行い、`.nanikaai/config.json` が `is_sensitive_path` で拒否されること | `behavior_spec` / `config_store` |
| `SPEC-UI-06` | 簡潔メニュー＆ネイティブ設定パネル | 右クリックメニューを初期 `<= 20` 行の折りたたみ形式（`Skins` / `Voice` / `AI` / `Tools` インライン展開）に整理し、トレイも 4 サブメニュー構成としたうえで、`wgpu` ネイティブ設定パネル（`540x476`、4 タブ）および `GET/POST /api/settings` により透明度・拡大率・ホバー不透明・クリック透過・位置固定・OS 別便利設定を即時反映する | `--smoke-test` Phase E7 で折りたたみ行数 `<= 20`・アコーディオン展開・設定パネル描画 (`> 50,000 px`)・スライダー操作に伴う実ウィンドウ透明度同期・初期値復元・アプリ非終了閉じを通過し、`SettingsPanelModel` 4 タブ描画と `/api/settings`（`api_key` 非露出・範囲外 `400 Bad Request`）が合格すること | `--smoke-test` Phase E7 / `behavior_spec` |
| `SPEC-UI-07` | 多言語 UI 切替（OS 言語追従＆未対応時英語） | UI 言語設定 `ui_language` を既定 `"auto"`（OS 表示言語に自動追従、未対応ロケールは英語 `UiLang::En` へフォールバック）とし、設定パネル Tab 1・右クリックメニュー `[🛠 ツール・言語]`・トレイ・`POST /api/settings` から `"auto" \| "ja" \| "ko" \| "en"` を再起動不要で即時切替でき、`GET /api/settings` に `ui_language` と `effective_ui_language` を返し、既定値リセットで `"auto"` へ復元すること | `AgentConfig::default().ui_language == "auto"`、`resolve_ui_lang_with_os` で `ja-JP`→`Ja` / `ko-KR`→`Ko` / `en-US`→`En` / 未対応ロケール(`fr-FR` 等)→`En`、`POST /api/settings` の `"ko"` 受理 (`200`)・`"fr"` 拒否 (`400`)、`Ja`/`Ko`/`En` 全 4 タブ描画 (`> 50,000 px`)、および `--smoke-test` Tab 1 連続切替・リセットが通過すること | `unit tests` / `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-ACC-01` | おやすみモード・全画面自動退避・一時非表示 | `quiet_mode` で音声読み上げ・雑談・時報を一括ミュートし、`auto_hide_on_fullscreen` で macOS Spaces (`type=4`)・Linux (`_NET_WM_STATE_FULLSCREEN`)・Windows (`SHQueryUserNotificationState`) 検知時およびワンクリック一時非表示（トレイ・設定パネル・`/api/settings` `characters_hidden`）時に全ウィンドウを退避して GPU 描画ループを休止できること | 3 OS 全画面出力パーサーの正常判定（履歴スペース非誤爆含む）、設定永続化・既定値リセット、`/api/settings` の `pomodoro_action`/`power_save_mode` 不正値 `400` 拒否、および `--smoke-test` Phase E7 の非表示/復帰操作が通過すること | `unit tests` / `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-ACC-02` | ポモドーロ集中タイマー＆専用ポーズ・伴走通知 | 5 種のプリセット（`25/5`・`50/10`・`15/3`・`45/15`・`52/17`）＋ 4 サイクル毎のロングブレイク・フェーズスキップ・累計集中分を保持し、稼働中は専用ポーズ（`\0` 腕時計 `surface9`＋`\1` 巨大時計 `surface13` ＆視線追従）へ自動切替し、集中中は自動雑談・時報を自動抑制すること | `POMODORO_PRESETS.len() == 5`、4 サイクル完了時のロングブレイク切替、`neo_default` の `surface9`/`surface13` および Live2D `active_surfaces`/`hide_on_surfaces` 層読込、および `--smoke-test` Phase E7 操作が通過すること | `unit tests` / `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-ACC-03` | 発話バックログ＆ワンクリックコピー | 最新 30 件の発話履歴（時刻・`\0`/`\1` 本文・抽出 URL リンク）をリングバッファ保持し、吹き出し即時リプレイ（`\_q` ＋ `__copy_last_reply__` 選択肢）、ワンクリッククリップボードコピー、および `GET /api/backlog` JSON 取得を提供すること | `MAX_BACKLOG_ENTRIES=30` 上限超過時の FIFO 回転・連続重複抑止・URL 抽出、`\_a`/`\_q` リプレイ整形、`POST /api/speak` 後の `GET /api/backlog` 件数増加、および `--smoke-test` Phase E7 バックログ/コピー操作が通過すること | `unit tests` / `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-ACC-04` | OS ログイン自動起動・省電力・マルチモニタ移動 | macOS LaunchAgent plist・Linux `.desktop`・Windows `HKCU\...\Run` 登録の切替（テスト/スモーク時は実機設定不変）、`power_save_mode` (`auto`/`on`/`off`) ＋バッテリー検知（`<=20%` 低バッテリー労わり通知＆66ms 省電力ループ）、および次モニタ/左右端移動を提供すること | 3 OS 自動起動マニフェスト生成/削除の一時ディレクトリ検証、`pmset`/`/sys/class/power_supply`/Win バッテリー解析、`effective_poll_interval_ms` (`250`/`66`/`20ms`)、`select_next_monitor_index`、および `--smoke-test` Phase E7 操作が通過すること | `unit tests` / `behavior_spec` / `--smoke-test` Phase E7 |
| `SPEC-ACC-05` | クイック付箋メモ＆クリップボード質問・メニュー保護 | `~/.nanikaai/memos.json` による付箋 To-Do（AI ツール・吹き出し完了ボタン・`GET/POST /api/memos`）、機密文字列自動遮断付きワンクリッククリップボード質問（`<untrusted_data type="clipboard">` 隔離）、および右クリックメニュー表示中の自動雑談遅延保護を提供すること | `MemoStore` CRUD・完了・クリア、`sanitize_clipboard_for_ai` による API キー/秘密鍵遮断、およびメニュー展開時の 6 行簡潔音声メニューが正常に動作すること | `unit tests` / `behavior_spec` |
| `SPEC-COMP-06` | AivisSpeech モデル削除・自然な声色調整・試聴 | 設定パネル Tab 2（`🎤 音声・声色`）から標準モデル保護付きで追加導入済み AIVM モデルを `DELETE /aivm_models/{uuid}/uninstall` で削除でき、話速 (`0.75-1.35`)・自然さ (`0.70-1.50`) スライダーおよびワンクリック試聴を提供すること | `voice_speed`/`voice_intonation` 範囲クランプ、`uninstall_aivm_model_blocking` の非ローカル URL・不正 UUID 拒否、および Tab 2 描画 (`> 50,000 px`) が通過すること | `unit tests` / `behavior_spec` |
| `SPEC-ACC-06` | マルチカレンダー移動時間出発通知＆チャット画像添付 | Google・OS 標準（macOS Calendar / Windows Outlook / Linux）・Apple iCloud/iCal (`webcal://`)・Yahoo! Japan・Yahoo・ローカル `.ics` を統合同期し、移動要否（オンライン会議除外）と移動時間＋準備時間を逆算した出発前通知・定期リマインド・`GET/POST /api/calendar`・AI ツール `list_upcoming_events` を提供し、さらにチャット欄の `[🖼]` ボタンおよび画像 D&D で PNG/JPG/BMP を安全縮小・Base64 化して Ollama / Gemini / OpenAI / Claude へ送信できること | `webcal://`→`https://` 正規化・SSRF 遮断、各プロバイダ識別、OS 標準 TSV / ICS 解析、出発前通知重複抑止、`load_image_attachment` の縮小・Base64 化・機密パス拒否が通過すること | `unit tests` / `behavior_spec` |

---

## 3. 検証実行・結果レポートおよび自動トリガー機構

- **手動・任意実行（Phase 0 事前検査 ＋ 検証 ＋ Markdown/JSON レポート生成）**:
  ```bash
  # Phase 0 事前機密・信頼境界コード検査のみ（テスト前チェック / < 0.2秒）
  python3 scripts/verify_behavior.py --pretest-only

  # 変更規模に応じて自動判定（Phase 0 通過後に fast / standard / full を実行）
  python3 scripts/verify_behavior.py --mode auto

  # 強制フル検証 ＋ レポートファイル保存
  python3 scripts/verify_behavior.py --mode full --report-md artifacts/verify_report.md --report-json artifacts/verify_report.json

  # 3 OS 実機横断検証（ローカル macOS + SSH 経由 Linux + SSH 経由 Windows 実機）
  python3 scripts/verify_behavior.py --mode full --remote-linux <linux_ssh_host> --remote-win <win_ssh_host>

  # 配布バイナリ単体スモーク検証（ビルド環境のない端末向け）
  python3 scripts/verify_behavior.py --mode full --binary <path_to_nanikaai_exe>
  ```
- **ローカル自動実行（Git `pre-commit` / `pre-push` フック）**:
  - `bash scripts/install_hooks.sh` で [`.githooks/pre-commit`](../.githooks/pre-commit)（`verify_behavior.py --pretest-only`）および [`.githooks/pre-push`](../.githooks/pre-push)（Phase 0 ＋ `verify_behavior.py --mode auto`）を有効化すると、コミット前とプッシュ前に個人情報・機密・指示境界違反および動作仕様を自動検査する。
- **CI 自動実行（GitHub Actions）**:
  - [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) が `main` へのプッシュ・PR ごとにまず **Phase 0 事前機密・信頼境界コード検査** を実行したうえで、文書のみなら高速静的チェック、コード変更時は macOS / Ubuntu / Windows で `verify_behavior.py` を自動実行し、GitHub Actions Step Summary に合否レポートを出力する。
