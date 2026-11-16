# 何か藍 (`NanikaAi`) — 現行動作仕様および自動検証基準 (`BEHAVIOR_SPEC`)

- **Document Version**: `v1.0.0`
- **Target Software**: `nanikaai` `v0.1.3+`
- **Purpose**: 改修後の自動動作確認テスト（`cargo test` / `--smoke-test` / 静的監査）における合否基準と、変更内容に応じた検証ティア・結果報告様式を定義する。

---

## 1. 変更規模別の自動検証ティア規約（軽微修正の扱い）

毎回不要なフルビルドを行わず、修正内容に応じて検証範囲を自動選択する（`python3 scripts/verify_behavior.py --mode auto`）。

| ティア (`--mode`) | 適用条件（自動判定基準） | 実施内容 |
| :--- | :--- | :--- |
| **Tier 0: `fast`** | 文書のみの変更（`*.md`・`LICENSE`・`.gitignore` のみ、ただし `assets/` 配下を除く） | 再ビルド不要の高速静的検査（`#![forbid(unsafe_code)]` 維持、ライセンス分離確認、残留マーカー `0` 件監査）を実施する。 |
| **Tier 1: `standard`** | ロジック・サーバー・AI・パーサー・テスト・設定・CI 定義の変更 | Tier 0 ＋ 単体テスト全件および [`behavior_spec.rs`](../tests/behavior_spec.rs) 統合仕様テスト一式（`cargo test`）を実施する。 |
| **Tier 2: `full`** | GUI (`src/gui.rs`)・シェル描画 (`src/shell_parser.rs`)・ビルドスクリプト・`assets/` の変更（非 CI のデスクトップ環境）、またはリリース前手動実行 | Tier 1 ＋ 必要時リリースビルド ＋ 実ウィンドウ/GPU スモーク検証（`--smoke-test`）＋ 稼働中ローカル API ヘルス確認を実施する。 |

---

## 2. 現行動作仕様および判定基準一覧（20 項目）

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
| `SPEC-SRV-01` | HTTP 状態 API | `GET /api/health`・`/api/skins`・`/api/providers` が JSON (`ok: true`) を返す | 一時的 `start_dual_server(0, 0)` に対して全 3 エンドポイントが `200 OK` かつ `"ok": true` を返すこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-02` | HTTP 操作 API | `POST /api/speak`・`/api/chat`・`/api/event`・`/api/skin`・`/api/provider` を処理する | 正常ペイロードで `200 OK`、必須項目欠落（空 `script`/`message`/`provider`）で `400 Bad Request` を返すこと | `behavior_spec` |
| `SPEC-SRV-03` | SSTP 互換 | HTTP `/sstp` および生 TCP `SSTP/1.4` 要求を受理する | HTTP `/sstp` が `200 OK`＋`X-Actions`、TCP `SSTP/1.4` が `200 OK`＋`Charset: UTF-8` を返し、空 `Script` に `400 Bad Request` を返すこと | `behavior_spec` / `--smoke-test` |
| `SPEC-SRV-04` | 自動監査・保守 | 個人パス・機密・未処理マーカーをリポジトリへ混入させない | 追跡対象ファイルに未処理の作業待ち・仮置き注記やローカル個人パスが `0` 件であること | `verify_behavior.py` |

---

## 3. 検証実行・結果レポートおよび自動トリガー機構

- **手動・任意実行（検証＋Markdown/JSON レポート生成）**:
  ```bash
  # 変更規模に応じて自動判定（文書のみなら fast、ロジックは standard、GUI/資産は full）
  python3 scripts/verify_behavior.py --mode auto

  # 強制フル検証 ＋ レポートファイル保存
  python3 scripts/verify_behavior.py --mode full --report-md artifacts/verify_report.md --report-json artifacts/verify_report.json
  ```
- **ローカル自動実行（Git `pre-push` フック）**:
  - `bash scripts/install_hooks.sh` で [`.githooks/pre-push`](../.githooks/pre-push) を有効化すると、`git push` ごとに `verify_behavior.py --mode auto` が自動実行され、不合格時はプッシュ前に停止する。
- **CI 自動実行（GitHub Actions）**:
  - [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) が `main` へのプッシュ・PR ごとに変更内容を自動判定し、文書のみなら高速静的チェック、コード変更時は macOS / Ubuntu / Windows で `verify_behavior.py` を自動実行し、GitHub Actions Step Summary に合否レポートを出力する。
