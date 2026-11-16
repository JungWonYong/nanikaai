# 何か藍 (`NanikaAi`) — 現行動作仕様および自動検証基準 (`BEHAVIOR_SPEC`)

- **Document Version**: `v1.5.0`
- **Target Software**: `nanikaai` `v0.1.5+`
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

## 2. 現行動作仕様および判定基準一覧（31 項目）

| 仕様 ID | 領域 | 期待される現行動作（仕様） | 合格基準（パス条件） | 検証手段 |
| :--- | :--- | :--- | :--- | :--- |
| `SPEC-SAFE-01` | 安全性 | 100% Safe Rust を厳守する | `src/lib.rs` と `src/main.rs` 先頭に `#![forbid(unsafe_code)]` が存在し、`src/` に `unsafe` 構文が `0` 件であること | `verify_behavior.py` / `behavior_spec` |
| `SPEC-SAFE-02` | 権利分離 | プログラムは `MIT`、同梱既定スキンは `CC0-1.0` に分離する | `LICENSE`、`neo_default/LICENSE-CC0.txt`、`clean_default/LICENSE-CC0.txt`、`docs/BEHAVIOR_SPEC.md` が存在し非空であること | `verify_behavior.py` / `behavior_spec` |
| `SPEC-SAFE-03` | ツール保護 | 機密ファイル参照と SSRF（非公開 IP・ローカル帯域アクセス）を遮断する | `is_sensitive_path` が `.ssh`/`.aws`/`credentials*.json`/`.env` を拒否し、`validate_public_url` が `127.0.0.1`/`169.254.169.254`/プライベート IP(`10.x`/`172.16.x` 等)/`file://` を拒否すること | `behavior_spec` / unit tests |
| `SPEC-SKIN-01` | 既定優先順位 | 同梱既定スキンを一覧の先頭に固定し、いつでも復帰可能とする | `ShellManager::list_shells()[0].id == "neo_default"` かつ `BalloonManager::list_balloons()[0].id == "clean_default"` であること | `behavior_spec` / `--smoke-test` |
| `SPEC-SKIN-02` | スコープ分離 | `\0`（メイン）と `\1`（パートナー）のサーフェス解決と表示を分離する | `resolve_surface_id(Scope::Kero, 5) >= 10` を満たし、`neo_default` の `\0`(surface `0`) と `\1`(surface `10`) が異なる非空 RGBA 画像であること | `behavior_spec` / `--smoke-test` |
| `SPEC-SKIN-03` | HiDPI・SERIKO | `neo_default` を `@2x` 解像度と目パチ/発話アニメーション付きで読み込む | `descript.display_scale` が `2.0` であり、surface `0` と `10` の双方に `talk`/`always` 等の SERIKO アニメーションが定義されること | `behavior_spec` / unit tests |
| `SPEC-SKIN-04` | クラシック互換 | `.pna` アルファマスク透過と `.nar` パストラバーサル防御が機能する | `.pna` で `(0,0,0)` 黒衣装が不透明保持・背景が `alpha == 0` になり、`../../` を含む `.nar` 導入で親ディレクトリ外へ書込まないこと | `behavior_spec` / `--smoke-test` |
| `SPEC-DLG-01` | スクリプト解析 | 話者切替・表情・選択肢・一時停止・早期 `\e` 回復を正確に解釈する | `\0` / `\1` / `\s[n]` / `\q[label,id]` / `\wN` / `\e` が対応する `ActionKind` へ変換され、早期 `\e` 後の発話も失われないこと | `behavior_spec` / unit tests |
| `SPEC-DLG-02` | タグ・文字浄化 | LLM 思考タグ・バイトトークン・HTML 残骸・未対応制御タグを発話へ出さない | `<|thought|>...<|/thought|>`、`<end_of_turn>`、`<0xE3><0x80><0x80>` が `U+3000` 復元または完全除去されること | `behavior_spec` / unit tests |
| `SPEC-DLG-03` | 履歴保護 | 会話履歴保存時に `\1` スコープ不整合と制御タグを再帰浄化する | `sanitize_reply_for_history` が `\1\s[5]` を `\1` 側サーフェス(`12`)へ補正し `<|thought|>` 等を残さないこと | `behavior_spec` / unit tests |
| `SPEC-AI-01` | モデル規制 | 制限対象 LLM（`FORBIDDEN_MODEL_PREFIXES`）の呼び出しを拒否する | 禁止プレフィックス全種で `assert_allowed_model` が `Err` を返し、`PREFERRED_MODELS` に禁止モデルが含まれないこと | `behavior_spec` / unit tests |
| `SPEC-AI-02` | Ollama 適応 | 初心者向けローカルモデル候補を幅広に自動選択する | `PREFERRED_MODELS` に `gemma4:e4b`・`llama3.2:3b` 等が含まれ、`provider_catalog()` の `"ollama"` 既定モデルが `"auto"` であること | `behavior_spec` / unit tests |
| `SPEC-AI-03` | 設定永続化 | `config.json` と記憶データをディスクへ保存・再読込できる | `ConfigStore::update(ConfigPatch)` 後の値が新規 `ConfigStore::new` インスタンスで一致すること | `behavior_spec` / unit tests |
| `SPEC-UI-01` | GPU/Live2D 基盤 | シェーダ定数レイアウトと減衰バネ物理・足元固定表示が破綻しない | `ShaderParams` が `96 B`（WGSL 整合）であり、スモーク検証で `neo_default` の足元・短文吹き出し・スクロール長文・メニューを正常描画すること | unit tests / `--smoke-test` |
| `SPEC-UI-02` | クロス OS 構成 | macOS / Windows ではトレイ統合し、Linux は右クリックメニューで完結する | `tray_icon` 依存が `macos`/`windows` のみに限定され、Linux/Windows/macOS 全ターゲットでコンパイル可能であること | CI / `cargo test` |
| `SPEC-UI-03` | リリース版数 | `Cargo.toml` と配布バンドル版数が一致し、文書過負荷更新を避ける | `Cargo.toml` の版数と `scripts/build_macos.sh` のバンドル版数が一致し、`scripts/build_macos.sh` が正常終了すること | `verify_behavior.py --mode full` |
| `SPEC-UI-04` | GUI 対話操作性 | 右クリックメニュー展開/ホバー/スクロール/切替実行、キャラダブルクリック入力バー展開、IME入力/編集/送信(`↑`)/`×`/`Esc`、吹き出し早送り/選択肢ホバー/スクロール/クリック非表示が正常に動作する | `--smoke-test` Phase E において全ウィンドウ対話操作・ヒット判定・状態遷移が 1 件も欠落せず合格すること | `--smoke-test` Phase E (`SPEC-UI-04`) |
| `SPEC-UI-05` | 3秒以内反応性 SLA | ユーザー操作（送信・選択肢・右クリック・入力バー・制御API・オフライン縮退）から 3 秒（`3,000 ms`）以内に視覚反応または応答を返す | 送信/選択直後の「AI 応答準備中…」即時表示（`< 500 ms` ステータス通知）、GUI 全操作合計 `ui_ms <= 3,000 ms`、HTTP/TCP 制御応答 `ipc_ms <= 3,000 ms`、オフライン即断 `< 3,000 ms` を実測で満たすこと | `--smoke-test` Phase E / `behavior_spec` |
| `SPEC-SRV-01` | HTTP 状態 API | `GET /api/health`・`/api/skins`・`/api/providers` が JSON (`ok: true`) を返す | 一時的 `start_dual_server(0, 0)` に対して全 3 エンドポイントが `200 OK` かつ `"ok": true` を返すこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-02` | HTTP 操作 API | `POST /api/speak`・`/api/chat`・`/api/event`・`/api/skin`・`/api/provider` を処理する | 正常ペイロードで `200 OK`、必須項目欠落（空 `script`/`message`/`provider`）で `400 Bad Request` を返すこと | `behavior_spec` |
| `SPEC-SRV-03` | SSTP 互換 | HTTP `/sstp` および生 TCP `SSTP/1.4` 要求を受理する | HTTP `/sstp` が `200 OK`＋`X-Actions`、TCP `SSTP/1.4` が `200 OK`＋`Charset: UTF-8` を返し、空 `Script` に `400 Bad Request` を返すこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-04` | 自動監査・保守 | 個人パス・機密・未処理マーカーをリポジトリへ混入させない | 追跡対象ファイルに未処理の作業待ち・仮置き注記やローカル個人パスが `0` 件であること | `verify_behavior.py` |
| `SPEC-SEC-01` | 事前機密検査 | テスト・ビルド・リモート転送前に個人情報・APIキー・認証情報・秘密鍵の混入を遮断する | Phase 0 が全対象ファイルおよび Git 履歴から個人パス・非公開 LAN IP・個人メール・API キー・認証ファイル（`.env`/証明書）・秘密鍵 `0` 件を確認し、合成秘密サンプルをテスト前停止（Exit Code `!= 0`）で遮断すること | `verify_behavior.py` Phase 0 / `behavior_spec` |
| `SPEC-SEC-02` | AI指示境界 | AI への指示は現行憲法・指定設定パスのみに限定し、他経路コード・外部入力をデータとして隔離する | 未承認のリポジトリ内 AI 指示・フック・ビルド実行ファイルが `0` 件であり、`AgentEngine` が外部イベント・ツール出力・長期記憶を `<untrusted_data>` で隔離しスキンメタデータ改行/タグを無害化すること | `verify_behavior.py` Phase 0 / `behavior_spec` |
| `SPEC-OS-01` | マルチ OS 実機互換 | macOS / Linux / Windows の各実機環境で動作検証可能であること | ローカル（macOS）に加え、Linux 実機での `cargo test`＋`--smoke-test`、および Windows 実機での配布バイナリ `--smoke-test`（`[SMOKE PASS] unsafe=0`）が通過すること | `verify_behavior.py --remote-linux/--remote-win` / CI |
| `SPEC-GHOST-01` | スキン連動ゴースト切替 | スキン（シェル）切替に応じて人格・記憶・会話履歴をシェル単位で分離し、交代時挨拶および `.nar` の `ghost.name` 継承を行う | シェル切替前後で他シェルの長期記憶・会話履歴が混線せず、`ghost/master/descript.txt` の `name` が `ghost.name` として引き継がれること | `behavior_spec` / `unit tests` |
| `SPEC-SKIN-05` | 着せ替え (`bind`) | `descript.txt` の `bindgroup` と `surfaces.txt` の `interval,bind` を合成・切替・永続化する | `neo_default` 既定状態（OFF）は無改変と一致し、トグル ON で合成不透明ピクセル数が増加し、状態が設定へ永続化されること | `behavior_spec` / `--smoke-test` Phase E |
| `SPEC-DLG-04` | リンク・即時表示 | `\_q` 一括即時表示および `\_a[url]label\_a`・裸 URL のクリック可能リンクを抽出し安全検証のうえ開く | `\_q` 区間が 1 tick で完了し、`validate_public_url` 通過の公開 URL のみ（`dry_run` 時は起動なし）許可し非公開 URL を遮断すること | `behavior_spec` / `--smoke-test` Phase E |
| `SPEC-COMP-01` | 時報・リマインダー・終了挨拶 | 毎時 00 分の時報、`set_reminder` / `list_reminders` 通知、および終了時（`OnClose`）挨拶を実行する | ツール登録したリマインダーが期日到達で消化通知され、時報・終了挨拶スクリプトが現在キャラクター名で生成されること | `behavior_spec` / `agent_engine` |
| `SPEC-COMP-02` | ファイル D&D 反応 | `.nar`/`.zip` 以外の汎用ファイル（文章・コード・画像・URL ショートカット・フォルダ）のドロップ内容を受理し要約反応する | `.env`/秘密鍵/資格情報ファイルを読込前に即拒否し、安全ファイル内容のみ `<untrusted_data>` で囲んで抽出すること | `behavior_spec` / `companion_extras` |
| `SPEC-COMP-03` | 音声読み上げ・更新確認 | 既定 OFF の AivisSpeech (`:10101`) / VOICEVOX (`:50021`)＋OS TTS フォールバックおよび GitHub Releases 更新確認を提供する | `\0`/`\1` バッチ発話抽出および SemVer 版数比較が正確に機能し、スモーク検証時は外部起動・音声ブロックを発生させないこと | `behavior_spec` / `companion_extras` |
| `SPEC-COMP-04` | ヘッドレス音声自動起動 | 導入済み `AivisSpeech Engine` / `VOICEVOX vv-engine` を手動起動不要で自動検出・無画面起動/終了し、`/speakers` から `\0`/`\1` 話者選択をシェル別保存する | `/speakers` の 32bit/整数スタイルID解析と既定 `\0`/`\1` ペア選出およびシェル別 `sakura_speaker_id`/`kero_speaker_id` 永続化が一致すること | `behavior_spec` / `companion_extras` |

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
