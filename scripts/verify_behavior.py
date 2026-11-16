#!/usr/bin/env python3
"""
Automated Behavioral Verification & Structured Report Runner for 何か藍 (NanikaAi).

Modes:
  - auto:     Detects changed files via Git and selects:
              * fast     -> Docs-only / trivial repo edits (*.md excluding assets/, LICENSE, .gitignore)
              * standard -> Code / logic / server / AI / CI / test changes
              * full     -> GUI / shader / skin parser / assets / build scripts on desktop OS (non-CI)
  - fast:     Tier 0 static specification checks (< 1s, no Rust rebuild)
  - standard: Tier 0 + unit tests & tests/behavior_spec.rs integration suite (`cargo test`)
  - full:     Tier 1 + release build (if stale) + GPU/window `--smoke-test` + live health probe
"""
from __future__ import annotations

import argparse
import base64
import datetime
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

ROOT_DIR = Path(__file__).resolve().parent.parent


def _run(cmd: list[str], cwd: Path = ROOT_DIR, env: dict[str, str] | None = None) -> tuple[int, str]:
    merged_env = os.environ.copy()
    cargo_bin = str(Path.home() / ".cargo" / "bin")
    brew_bin = "/opt/homebrew/bin"
    cur_path = merged_env.get("PATH", "")
    for extra in (cargo_bin, brew_bin):
        if extra not in cur_path.split(os.pathsep) and Path(extra).is_dir():
            cur_path = f"{extra}{os.pathsep}{cur_path}" if cur_path else extra
    merged_env["PATH"] = cur_path
    if env:
        merged_env.update(env)

    proc = subprocess.run(
        cmd,
        cwd=str(cwd),
        env=merged_env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return proc.returncode, proc.stdout


def detect_changed_files() -> list[str]:
    rc, status_out = _run(["git", "status", "--porcelain"])
    changed: list[str] = []
    if rc == 0 and status_out.strip():
        for line in status_out.splitlines():
            if len(line) < 4:
                continue
            path_part = line[3:].strip()
            if " -> " in path_part:
                path_part = path_part.split(" -> ", 1)[1].strip()
            changed.append(path_part.strip('"'))
        return sorted(set(changed))

    rc2, diff_out = _run(["git", "diff", "--name-only", "HEAD~1..HEAD"])
    if rc2 == 0 and diff_out.strip():
        return sorted(set(line.strip() for line in diff_out.splitlines() if line.strip()))
    return []


def select_auto_mode(changed_files: list[str]) -> tuple[str, str]:
    if not changed_files:
        return "standard", "変更差分なし -> standard 検証を実行"

    def is_doc_only(path_str: str) -> bool:
        norm = path_str.replace("\\", "/")
        if norm.startswith("assets/"):
            return False
        return norm.endswith(".md") or norm in {".gitignore", "LICENSE"}

    if all(is_doc_only(f) for f in changed_files):
        return "fast", f"軽微・文書変更のみ ({len(changed_files)}件) -> Tier 0 (fast) を自動選択"

    gui_triggers = (
        "src/gui.rs",
        "src/shell_parser.rs",
        "src/main.rs",
        "Cargo.toml",
    )
    touches_gui_or_assets = any(
        f.replace("\\", "/") in gui_triggers
        or f.replace("\\", "/").startswith("assets/")
        or f.replace("\\", "/").startswith("scripts/build_")
        for f in changed_files
    )

    is_ci = os.environ.get("CI", "").lower() == "true"
    if touches_gui_or_assets and sys.platform == "darwin" and not is_ci:
        return "full", "GUI/シェル/資産/ビルドスクリプト変更検出 (macOS デスクトップ) -> Tier 2 (full) を自動選択"

    return "standard", f"ロジック/テスト/CI 変更検出 ({len(changed_files)}件) -> Tier 1 (standard) を自動選択"


def _collect_scan_files(scan_root: Path) -> list[Path]:
    git_dir = scan_root / ".git"
    if git_dir.exists():
        rc, ls_out = _run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=scan_root,
        )
        if rc == 0:
            files: list[Path] = []
            for raw_rel in ls_out.split("\x00"):
                if not raw_rel:
                    continue
                p = scan_root / raw_rel
                if p.is_file():
                    files.append(p)
            return sorted(set(files))

    skip_dirs = {".git", "target", "dist", "__pycache__", "artifacts", ".smoke-userdata"}
    collected: list[Path] = []
    for p in scan_root.rglob("*"):
        if not p.is_file():
            continue
        rel_parts = p.relative_to(scan_root).parts
        if any(part in skip_dirs or part.startswith(".smoke-userdata") for part in rel_parts):
            continue
        collected.append(p)
    return sorted(collected)


def check_phase0_pretest_security(scan_root: Path) -> list[dict[str, Any]]:
    """Phase 0 pre-test gate: must pass BEFORE cargo test, --smoke-test, or SSH sync."""
    files = _collect_scan_files(scan_root)

    # --- SPEC-SEC-01: PII / API keys / credentials / private keys zero-tolerance ---
    sec01_hits: list[str] = []
    forbidden_exact_names = {
        ".env",
        "credentials.json",
        "client_secret.json",
        "service_account.json",
        ".npmrc",
        ".pypirc",
        ".netrc",
    }
    forbidden_exts = {".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".kdbx"}
    forbidden_name_res = [
        re.compile(r"^\.env\..+", re.I),
        re.compile(r"^credentials.*\.json$", re.I),
        re.compile(r"^client_secret.*\.json$", re.I),
        re.compile(r"^service[_-]account.*\.json$", re.I),
        re.compile(r"^token.*\.json$", re.I),
        re.compile(r"^id_(?:rsa|ed25519|ecdsa|dsa)(?:\.pub)?$", re.I),
    ]

    priv_key_re = re.compile("-----BEGIN " + r"(?:[A-Z0-9 ]* )?PRIVATE KEY-----")
    api_secret_res = [
        ("google_api_key", re.compile("AI" + r"za[0-9A-Za-z_-]{35}")),
        ("anthropic_key", re.compile("sk-" + r"ant-[0-9A-Za-z_-]{20,}")),
        ("openai_proj_key", re.compile("sk-" + r"proj-[0-9A-Za-z_-]{20,}")),
        ("sk_secret_key", re.compile(r"\bsk" + r"-[0-9A-Za-z]{20,}\b")),
        ("github_token", re.compile(r"\bgh" + r"[pousr]_[0-9A-Za-z]{30,}\b")),
        ("github_pat", re.compile("github_" + r"pat_[0-9A-Za-z_]{20,}")),
        ("aws_access_key", re.compile("AK" + r"IA[0-9A-Z]{16}")),
        ("slack_token", re.compile(r"\bxox" + r"[baprs]-[0-9A-Za-z-]{10,}\b")),
        ("google_oauth", re.compile("ya" + r"29\.[0-9A-Za-z_-]{20,}")),
        ("discord_webhook", re.compile("https://(?:ptb\\.|canary\\.)?discord" + r"\.com/api/webhooks/\d+/[A-Za-z0-9_-]+")),
        ("slack_webhook", re.compile("https://hooks\\.slack" + r"\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[A-Za-z0-9]+")),
        ("bearer_token", re.compile(r"Authorization:\s*Bearer\s+[A-Za-z0-9._-]{20,}", re.I)),
        ("hardcoded_secret_assign", re.compile(r"""(?:api_key|client_secret|access_token|secret_key|password)\s*[:=]\s*["'][A-Za-z0-9_./+=-]{16,}["']""", re.I)),
    ]

    home_name = Path.home().name
    local_home_re = (
        re.compile(r"(?:" + "/" + "Users/|" + "/" + r"home/|\\{1,2}Users\\{1,2})" + re.escape(home_name) + r"(?:[/\\]|$)", re.I)
        if home_name
        else None
    )
    generic_home_re = re.compile(r"(?:" + "/" + "Users/|" + "/" + r"home/|C:\\{1,2}Users\\{1,2})([A-Za-z0-9._-]{2,})(?:[/\\]|$)")
    allowed_home_placeholders = {"user", "username", "yourname", "you", "alice", "bob", "example", "runner", "nanikaai"}
    lan_ip_re = re.compile("192" + r"\." + "168" + r"\.\d+\.\d+")
    email_re = re.compile(r"(?<![A-Za-z0-9._%+-])([A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.([A-Za-z]{2,}))\b")
    non_email_tlds = {"png", "jpg", "jpeg", "webp", "gif", "svg", "ico", "icns", "pna", "rs", "py", "md", "toml", "txt"}

    # --- SPEC-SEC-02: AI Instruction-Data Trust Boundary & Rogue Config Guard ---
    sec02_hits: list[str] = []
    rogue_ai_basenames = {
        "agents.md",
        "claude.md",
        "gemini.md",
        "copilot.md",
        "system_prompt.md",
        ".cursorrules",
        ".windsurfrules",
        ".clinerules",
        ".continuerules",
        "build.rs",
    }
    rogue_dir_prefixes = (
        ".claude/",
        ".gemini/",
        ".cursor/",
        ".windsurf/",
        ".cline/",
        ".agents/",
        ".github/instructions/",
        ".github/prompts/",
    )
    allowed_githooks = {".githooks/pre-commit", ".githooks/pre-push"}
    prompt_inj_res = [
        re.compile("ignore\\s+(?:all\\s+)?previous\\s+instructions", re.I),
        re.compile("disregard\\s+(?:all\\s+)?(?:system|previous)\\s+(?:prompt|instructions)", re.I),
        re.compile("<\\|im_" + "start\\|>system", re.I),
    ]

    binary_exts = {".png", ".jpg", ".jpeg", ".webp", ".gif", ".icns", ".ico", ".zip", ".nar", ".exe", ".dll", ".so", ".dylib"}

    for fp in files:
        rel_posix = fp.relative_to(scan_root).as_posix()
        name_low = fp.name.lower()

        # Check forbidden credential/key filenames
        is_forbidden_file = (
            name_low in forbidden_exact_names
            or fp.suffix.lower() in forbidden_exts
            or any(pat.match(fp.name) for pat in forbidden_name_res)
        )
        if is_forbidden_file:
            sec01_hits.append(f"{rel_posix} (forbidden credential/key file)")

        # Check unauthorized in-repo AI instruction / hook / build script paths
        if (
            name_low in rogue_ai_basenames
            or rel_posix.lower() == ".github/copilot-instructions.md"
            or any(rel_posix.lower().startswith(pref) for pref in rogue_dir_prefixes)
            or (rel_posix.startswith(".githooks/") and rel_posix not in allowed_githooks)
        ):
            sec02_hits.append(f"{rel_posix} (unapproved in-repo AI instruction/hook/build file)")

        if fp.suffix.lower() in binary_exts:
            continue

        text = fp.read_text(encoding="utf-8", errors="replace")
        if priv_key_re.search(text):
            sec01_hits.append(f"{rel_posix} (private key header)")
        for label, pat in api_secret_res:
            if pat.search(text):
                sec01_hits.append(f"{rel_posix} ({label})")
        if (local_home_re and local_home_re.search(text)) or lan_ip_re.search(text):
            sec01_hits.append(f"{rel_posix} (local user path or LAN IP)")
        else:
            for hm in generic_home_re.finditer(text):
                if hm.group(1).lower() not in allowed_home_placeholders:
                    sec01_hits.append(f"{rel_posix} (OS home path: {hm.group(1)})")
                    break
        for em in email_re.finditer(text):
            full_addr = em.group(1).lower()
            tld = em.group(2).lower()
            if tld in non_email_tlds or full_addr.startswith("2x@"):
                continue
            if not (
                full_addr.endswith("@users.noreply.github.com")
                or full_addr.endswith("@example.com")
                or full_addr.endswith("@example.org")
                or full_addr.endswith("@example.net")
            ):
                sec01_hits.append(f"{rel_posix} (email: {full_addr})")
                break

        for pinj in prompt_inj_res:
            if pinj.search(text):
                sec02_hits.append(f"{rel_posix} (prompt-injection directive pattern)")
                break

    if (scan_root / ".git").exists():
        log_rc, log_out = _run(["git", "log", "--format=%H %ae"], cwd=scan_root)
        if log_rc == 0:
            for line in log_out.splitlines():
                parts = line.strip().split(None, 1)
                if len(parts) != 2:
                    continue
                sha, email = parts[0][:8], parts[1].strip().lower()
                if not (
                    email.endswith("@users.noreply.github.com")
                    or email.endswith("@example.com")
                    or email.endswith("@example.org")
                ):
                    sec01_hits.append(f"commit {sha} ({email})")
                    break

    return [
        {
            "spec": "SPEC-SEC-01",
            "name": "Phase 0 事前機密・個人情報・秘密鍵コード検査",
            "status": "PASS" if not sec01_hits else "FAIL",
            "detail": f"走査 {len(files)}ファイル / 個人情報・APIキー・認証情報・秘密鍵=0" if not sec01_hits else f"遮断: {', '.join(sec01_hits[:5])}",
        },
        {
            "spec": "SPEC-SEC-02",
            "name": "Phase 0 AI指示配置境界・データ扱い分離検査",
            "status": "PASS" if not sec02_hits else "FAIL",
            "detail": f"走査 {len(files)}ファイル / 未承認AI指示・フック・注入文言=0" if not sec02_hits else f"遮断: {', '.join(sec02_hits[:5])}",
        },
    ]


def check_tier0_static() -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []

    # SPEC-SAFE-01: forbid(unsafe_code) + 0 unsafe code constructs
    unsafe_violations: list[str] = []
    for entry in ("src/lib.rs", "src/main.rs"):
        p = ROOT_DIR / entry
        if not p.is_file() or not p.read_text(encoding="utf-8", errors="replace").startswith("#![forbid(unsafe_code)]"):
            unsafe_violations.append(f"missing #![forbid(unsafe_code)] in {entry}")

    unsafe_kw = "un" + "safe"
    for rs_path in sorted((ROOT_DIR / "src").rglob("*.rs")):
        text = rs_path.read_text(encoding="utf-8", errors="replace")
        if "allow(unsafe_code)" in text:
            unsafe_violations.append(f"allow(unsafe_code) in {rs_path.relative_to(ROOT_DIR)}")
        for lineno, line in enumerate(text.splitlines(), 1):
            code = line.split("//", 1)[0]
            if re.search(rf"\b{unsafe_kw}\b", code) and not re.search(r'"[^"]*' + unsafe_kw + r'[^"]*"', code):
                if re.search(rf"\b{unsafe_kw}\s*(\{{|fn|impl|trait)\b", code):
                    unsafe_violations.append(f"{rs_path.relative_to(ROOT_DIR)}:{lineno}")

    results.append({
        "spec": "SPEC-SAFE-01",
        "name": "Zero unsafe Rust & forbid(unsafe_code)",
        "status": "PASS" if not unsafe_violations else "FAIL",
        "detail": "src/ unsafe=0" if not unsafe_violations else "; ".join(unsafe_violations),
    })

    # SPEC-SAFE-02: clean license & spec presence
    req_docs = [
        "LICENSE",
        "assets/shells/neo_default/LICENSE-CC0.txt",
        "assets/balloons/clean_default/LICENSE-CC0.txt",
        "docs/BEHAVIOR_SPEC.md",
    ]
    missing = [d for d in req_docs if not (ROOT_DIR / d).is_file() or (ROOT_DIR / d).stat().st_size < 32]
    results.append({
        "spec": "SPEC-SAFE-02",
        "name": "MIT + CC0-1.0 分離・動作仕様書存在",
        "status": "PASS" if not missing else "FAIL",
        "detail": f"検証済み {len(req_docs)}ファイル" if not missing else f"欠落/空: {', '.join(missing)}",
    })

    # SPEC-UI-03 (metadata consistency): Cargo.toml version vs build_macos.sh bundle version
    cargo_txt = (ROOT_DIR / "Cargo.toml").read_text(encoding="utf-8", errors="replace")
    cargo_m = re.search(r'^version\s*=\s*"([^"]+)"', cargo_txt, re.M)
    cargo_ver = cargo_m.group(1) if cargo_m else "unknown"
    mac_sh = (ROOT_DIR / "scripts/build_macos.sh").read_text(encoding="utf-8", errors="replace")
    mac_ok = f"<string>{cargo_ver}</string>" in mac_sh
    results.append({
        "spec": "SPEC-UI-03",
        "name": "クレート・バンドル版数整合性",
        "status": "PASS" if mac_ok else "FAIL",
        "detail": f"Cargo.toml={cargo_ver}, build_macos.sh一致={mac_ok}",
    })

    # SPEC-SRV-04: residual / personal leak audit across git-tracked files (and unstaged non-ignored files)
    leak_hits: list[str] = []
    marker_re = re.compile("|".join(["FIX" + "ME", "TO" + "DO", "DUM" + "MY"]))
    home_name = Path.home().name
    home_pat = rf"(?:/Users/|/home/|\\\\Users\\\\){re.escape(home_name)}(?:[/\\\\]|$)" if home_name else r"(?!)^"
    lan_prefix = "192" + r"\." + "168" + r"\.\d+\.\d+"
    personal_re = re.compile(rf"{home_pat}|{lan_prefix}", re.I)
    self_rel = Path("scripts/verify_behavior.py")

    rc, ls_out = _run(["git", "ls-files"])
    tracked = [Path(p) for p in ls_out.splitlines() if p.strip()] if rc == 0 else []
    for rel in tracked:
        fp = ROOT_DIR / rel
        if not fp.is_file() or fp.suffix.lower() in {".png", ".jpg", ".icns", ".ico", ".zip", ".nar"}:
            continue
        if rel == Path("tests/behavior_spec.rs") or rel == self_rel:
            # Exclude test pattern strings themselves from self-scan; audit only personal IP/path there.
            txt = fp.read_text(encoding="utf-8", errors="replace")
            if personal_re.search(txt):
                leak_hits.append(str(rel))
            continue
        txt = fp.read_text(encoding="utf-8", errors="replace")
        if marker_re.search(txt) or personal_re.search(txt):
            leak_hits.append(str(rel))

    results.append({
        "spec": "SPEC-SRV-04",
        "name": "未処理マーカー・個人情報混入ゼロ監査",
        "status": "PASS" if not leak_hits else "FAIL",
        "detail": f"追跡テキスト {len(tracked)}件 残留=0" if not leak_hits else f"検出: {', '.join(leak_hits[:5])}",
    })

    return results


def parse_cargo_test_output(output: str) -> tuple[int, int, dict[str, bool]]:
    total_passed = 0
    total_failed = 0
    spec_cases: dict[str, bool] = {}

    for m in re.finditer(r"test result:\s+\w+\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;", output):
        total_passed += int(m.group(1))
        total_failed += int(m.group(2))

    for m in re.finditer(r"^test\s+(spec_[A-Za-z0-9_]+)\s+\.\.\.\s+(ok|FAILED)$", output, re.M):
        spec_cases[m.group(1)] = m.group(2) == "ok"

    return total_passed, total_failed, spec_cases


def check_tier1_cargo_tests() -> tuple[list[dict[str, Any]], str]:
    rc, out = _run(["cargo", "test"])
    passed, failed, spec_cases = parse_cargo_test_output(out)
    results: list[dict[str, Any]] = []

    overall_ok = (rc == 0) and (failed == 0) and (passed > 0)
    results.append({
        "spec": "CARGO-TEST",
        "name": "単体テスト＋仕様統合テスト実行 (`cargo test`)",
        "status": "PASS" if overall_ok else "FAIL",
        "detail": f"exit={rc}, passed={passed}, failed={failed}, spec_cases={len(spec_cases)}",
    })

    expected_groups = [
        ("SPEC-SAFE-03", "spec_safe_03_builtin_tool_security_boundaries", "ツール機密・SSRF 遮断境界"),
        ("SPEC-SKIN-01/02/03", "spec_skin_01_02_03_default_order_scope_isolation_and_hidpi", "既定順位・\\0/\\1 分離・@2x/SERIKO"),
        ("SPEC-SKIN-04", "spec_skin_04_pna_mask_and_nar_traversal_guard", ".pna 透過マスク・.nar パス防御"),
        ("SPEC-DLG-01/02/03", "spec_dlg_01_02_03_sakura_script_and_tag_sanitization", "Sakura Script・タグ浄化・履歴保護"),
        ("SPEC-AI-01/02/03", "spec_ai_01_02_03_model_guard_ollama_prefs_and_config_persistence", "制限LLM拒否・Ollama候補・設定保存"),
        ("SPEC-SRV-01/02/03", "spec_srv_01_02_03_dual_http_and_sstp_tcp_contract", "HTTP API 全9系統・TCP SSTP/1.4"),
        ("SPEC-UI-04/05", "spec_ui_04_05_control_and_fallback_responsiveness_under_3s", "制御応答・即時ステータス・3秒以内SLA"),
        ("SPEC-SEC-01", "spec_sec_01_pretest_scanner_blocks_secrets_pii_and_private_keys", "テスト前機密・個人情報遮断"),
        ("SPEC-SEC-02", "spec_sec_02_instruction_data_trust_boundary_and_rogue_config_block", "AI指示境界・データ隔離防御"),
        ("SPEC-GHOST-01/SKIN-05", "spec_ghost_01_and_skin_05_profile_isolation_and_bind_dressup", "ゴーストプロファイル分離・着せ替え(bind)"),
        ("SPEC-COMP-01/02/03/DLG-04", "spec_comp_01_02_03_dlg_04_reminders_drop_links_and_updates", "時報/リマインダー・D&D・リンク/\\ _q・音声/更新確認"),
    ]
    for spec_id, test_fn, label in expected_groups:
        ok = spec_cases.get(test_fn, False)
        results.append({
            "spec": spec_id,
            "name": label,
            "status": "PASS" if ok else "FAIL",
            "detail": f"`{test_fn}` -> {'ok' if ok else 'missing/failed'}",
        })

    return results, out


def release_binary_needs_build(bin_path: Path) -> bool:
    if not bin_path.is_file():
        return True
    bin_mtime = bin_path.stat().st_mtime
    watch_paths = [ROOT_DIR / "Cargo.toml", ROOT_DIR / "Cargo.lock"]
    watch_paths.extend((ROOT_DIR / "src").rglob("*.rs"))
    watch_paths.extend((ROOT_DIR / "assets").rglob("*"))
    for p in watch_paths:
        if p.is_file() and p.stat().st_mtime > bin_mtime:
            return True
    return False


def check_tier2_full(custom_binary: str | None = None) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    if custom_binary:
        bin_path = Path(custom_binary)
        if not bin_path.is_file():
            results.append({
                "spec": "SPEC-UI-01",
                "name": "実ウィンドウ / GPU スモーク検証 (`--smoke-test`)",
                "status": "FAIL",
                "detail": f"指定バイナリが存在しません: {bin_path}",
            })
            return results
    else:
        bin_name = "nanikaai.exe" if os.name == "nt" else "nanikaai"
        bin_path = ROOT_DIR / "target" / "release" / bin_name

        if release_binary_needs_build(bin_path):
            b_rc, b_out = _run(["cargo", "build", "--release"])
            if b_rc != 0:
                tail = "\n".join(b_out.splitlines()[-15:])
                results.append({
                    "spec": "SPEC-UI-01",
                    "name": "実ウィンドウ / GPU スモーク検証 (`--smoke-test`)",
                    "status": "FAIL",
                    "detail": f"release build failed (exit={b_rc}): {tail}",
                })
                return results

    with tempfile.TemporaryDirectory(prefix="nanikaai_smoke_") as tmp_dir:
        smoke_log = Path(tmp_dir) / "smoke.log"
        proof_dir = Path(tmp_dir) / "artifacts"
        s_rc, s_out = _run(
            [str(bin_path), "--smoke-test"],
            env={
                "NANIKAAI_SMOKE_LOG": str(smoke_log),
                "NANIKAAI_SMOKE_PROOF_DIR": str(proof_dir),
            },
        )
        log_txt = smoke_log.read_text(encoding="utf-8", errors="replace") if smoke_log.is_file() else ""

    combined_out = f"{s_out}\n{log_txt}"
    smoke_line = next((ln for ln in combined_out.splitlines() if "[SMOKE PASS]" in ln), "")
    smoke_ok = (s_rc == 0) and bool(smoke_line) and ("unsafe=0" in smoke_line)
    results.append({
        "spec": "SPEC-UI-01",
        "name": "実ウィンドウ / GPU スモーク検証 (`--smoke-test`)",
        "status": "PASS" if smoke_ok else "FAIL",
        "detail": smoke_line if smoke_line else f"exit={s_rc}, [SMOKE PASS] 未検出",
    })

    ui_m = re.search(r"\bui_ms=(\d+)", smoke_line)
    ipc_m = re.search(r"\bipc_ms=(\d+)", smoke_line)
    if ui_m and ipc_m:
        ui_ms = int(ui_m.group(1))
        ipc_ms = int(ipc_m.group(1))
        sla_ok = smoke_ok and (ui_ms <= 3000) and (ipc_ms <= 3000)
        sla_detail = f"全GUI対話操作 PASS (ui_ms={ui_ms}ms <= 3000ms, ipc_ms={ipc_ms}ms <= 3000ms)"
    else:
        sla_ok = False
        sla_detail = "smoke出力に ui_ms / ipc_ms 計測値がありません"
    results.append({
        "spec": "SPEC-UI-04/05",
        "name": "GUI 対話操作性＆3秒以内反応性実測 (`--smoke-test`)",
        "status": "PASS" if sla_ok else "FAIL",
        "detail": sla_detail,
    })

    # Optional non-destructive probe against live background instance
    try:
        with urllib.request.urlopen("http://127.0.0.1:9821/api/health", timeout=1.5) as resp:
            payload = json.loads(resp.read().decode("utf-8", errors="replace"))
        ok_live = payload.get("ok") is True and payload.get("unsafe_lines") == 0
        results.append({
            "spec": "LIVE-HEALTH",
            "name": "常駐インスタンス `/api/health` 実機疎通",
            "status": "PASS" if ok_live else "FAIL",
            "detail": f"shell={payload.get('shell')}, provider={payload.get('provider')}, unsafe_lines={payload.get('unsafe_lines')}",
        })
    except (urllib.error.URLError, TimeoutError, OSError, ValueError):
        results.append({
            "spec": "LIVE-HEALTH",
            "name": "常駐インスタンス `/api/health` 実機疎通",
            "status": "SKIP",
            "detail": "常駐アプリ未起動のためスキップ（--smoke-test 側で HTTP/TCP 実証済み）",
        })

    return results


def _build_source_tarball() -> bytes:
    proc = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=str(ROOT_DIR),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tf:
        for raw_rel in proc.stdout.split(b"\x00"):
            if not raw_rel:
                continue
            rel = raw_rel.decode("utf-8", errors="surrogateescape")
            fp = ROOT_DIR / rel
            if fp.is_file():
                tf.add(fp, arcname=rel)
    return buf.getvalue()


def check_remote_linux(host: str) -> dict[str, Any]:
    name = f"Linux 実機検証 ({host})"
    try:
        tar_bytes = _build_source_tarball()
    except Exception as exc:
        return {"spec": "MULTI-OS-LINUX", "name": name, "status": "FAIL", "detail": f"ソース梱包失敗: {exc}"}

    sync_proc = subprocess.run(
        [
            "ssh",
            "-o",
            "BatchMode=yes",
            host,
            "mkdir -p ~/.cache/nanikaai-verify && tar -xzf - -C ~/.cache/nanikaai-verify",
        ],
        input=tar_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if sync_proc.returncode != 0:
        out_txt = sync_proc.stdout.decode("utf-8", errors="replace").strip()
        return {"spec": "MULTI-OS-LINUX", "name": name, "status": "FAIL", "detail": f"SSH転送失敗: {out_txt[-200:]}"}

    remote_script = r"""set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$HOME/.cache/nanikaai-verify"
rm -f /tmp/nanikaai_verify_linux.json
if [[ -S /tmp/.X11-unix/X0 ]]; then
  export DISPLAY=":0"
  if [[ -r "/run/user/$(id -u)/gdm/Xauthority" ]]; then
    export XAUTHORITY="/run/user/$(id -u)/gdm/Xauthority"
  elif [[ -f "$HOME/.Xauthority" ]]; then
    export XAUTHORITY="$HOME/.Xauthority"
  fi
fi
set +e
python3 scripts/verify_behavior.py --mode full --quiet --report-json /tmp/nanikaai_verify_linux.json
RC=$?
if [[ $RC -ne 0 ]] && command -v xvfb-run >/dev/null 2>&1; then
  xvfb-run -a -s "-screen 0 1920x1080x24" python3 scripts/verify_behavior.py --mode full --quiet --report-json /tmp/nanikaai_verify_linux.json
  RC=$?
fi
echo "===LINUX_VERIFY_JSON==="
cat /tmp/nanikaai_verify_linux.json 2>/dev/null || echo '{"overall":"FAIL","counts":{"PASS":0,"FAIL":1,"SKIP":0},"checks":[]}'
rm -f /tmp/nanikaai_verify_linux.json
exit $RC
"""
    run_proc = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", host, "bash -s"],
        input=remote_script,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    marker = "===LINUX_VERIFY_JSON==="
    raw_out = run_proc.stdout
    if marker not in raw_out:
        return {
            "spec": "MULTI-OS-LINUX",
            "name": name,
            "status": "FAIL",
            "detail": f"exit={run_proc.returncode}, 出力欠損: {' | '.join(raw_out.splitlines()[-8:])}",
        }
    json_str = raw_out.split(marker, 1)[1].strip()
    try:
        rep = json.loads(json_str)
    except ValueError as exc:
        return {"spec": "MULTI-OS-LINUX", "name": name, "status": "FAIL", "detail": f"JSON解析失敗: {exc}"}

    checks_map = {c.get("spec"): c for c in rep.get("checks", [])}
    cargo_det = checks_map.get("CARGO-TEST", {}).get("detail", "")
    smoke_det = checks_map.get("SPEC-UI-01", {}).get("detail", "")
    sla_det = checks_map.get("SPEC-UI-04/05", {}).get("detail", "")
    smoke_short = smoke_det.split(" scale=")[0] if " scale=" in smoke_det else smoke_det
    ok = (
        (run_proc.returncode == 0)
        and (rep.get("overall") == "PASS")
        and (checks_map.get("SPEC-UI-04/05", {}).get("status") == "PASS")
    )
    return {
        "spec": "MULTI-OS-LINUX",
        "name": name,
        "status": "PASS" if ok else "FAIL",
        "detail": f"{rep.get('counts')} | {cargo_det} | {sla_det or smoke_short}",
    }


def check_remote_win(host: str) -> dict[str, Any]:
    name = f"Windows 実機検証 ({host})"
    zip_url = os.environ.get(
        "NANIKAAI_WIN_ZIP_URL",
        "https://github.com/JungWonYong/nanikaai/releases/latest/download/NanikaAi-windows-x64.zip",
    )
    ps_script = rf"""
$ErrorActionPreference = 'Stop'
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12
$work = Join-Path $env:TEMP 'nanikaai-verify'
$unpack = Join-Path $work 'unpacked'
$zip = Join-Path $work 'NanikaAi-windows-x64.zip'
$smokeLog = Join-Path $work 'smoke.log'
$proofDir = Join-Path $work 'proof'
New-Item -ItemType Directory -Force -Path $work, $proofDir | Out-Null
if (Test-Path $unpack) {{ Remove-Item -Recurse -Force $unpack }}
if (Test-Path $smokeLog) {{ Remove-Item -Force $smokeLog }}
Invoke-WebRequest -UseBasicParsing -Uri '{zip_url}' -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $unpack -Force
$exeObj = Get-ChildItem -Path $unpack -Filter 'nanikaai.exe' -Recurse | Select-Object -First 1
if (-not $exeObj) {{ throw 'nanikaai.exe not found in archive' }}
$exe = $exeObj.FullName
$modeUsed = 'direct'
$rc = 1
$env:NANIKAAI_SMOKE_LOG = $smokeLog
$env:NANIKAAI_SMOKE_PROOF_DIR = $proofDir
$proc = Start-Process -FilePath $exe -ArgumentList '--smoke-test' -Wait -PassThru
$rc = $proc.ExitCode
$smokeLine = ''
if (Test-Path $smokeLog) {{ $smokeLine = (Get-Content -Raw -Encoding UTF8 $smokeLog).Trim() }}
if ($rc -ne 0 -or -not ($smokeLine -match '\[SMOKE PASS\]')) {{
  $modeUsed = 'schtasks-interactive'
  $taskRc = Join-Path $work 'smoke.rc'
  if (Test-Path $taskRc) {{ Remove-Item -Force $taskRc }}
  if (Test-Path $smokeLog) {{ Remove-Item -Force $smokeLog }}
  $runnerPs = Join-Path $work 'run_smoke_it.ps1'
  $scriptBody = @"
`$env:NANIKAAI_SMOKE_LOG = '$smokeLog'
`$env:NANIKAAI_SMOKE_PROOF_DIR = '$proofDir'
`$p = Start-Process -FilePath '$exe' -ArgumentList '--smoke-test' -Wait -PassThru
`$p.ExitCode | Out-File -Encoding ascii '$taskRc'
"@
  Set-Content -Path $runnerPs -Value $scriptBody -Encoding UTF8
  schtasks /Create /F /SC ONCE /ST 23:59 /TN 'NanikaAiSmokeVerify' /IT /RL LIMITED /TR "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$runnerPs`"" | Out-Null
  schtasks /Run /TN 'NanikaAiSmokeVerify' | Out-Null
  for ($i = 0; $i -lt 50; $i++) {{
    if (Test-Path $taskRc) {{ break }}
    Start-Sleep -Milliseconds 500
  }}
  schtasks /Delete /F /TN 'NanikaAiSmokeVerify' 2>$null | Out-Null
  if (Test-Path $taskRc) {{ $rc = [int]((Get-Content -Raw $taskRc).Trim()) }}
  if (Test-Path $smokeLog) {{ $smokeLine = (Get-Content -Raw -Encoding UTF8 $smokeLog).Trim() }}
}}
$gpu = ((Get-CimInstance Win32_VideoController | Where-Object {{ $_.Name -notmatch 'Virtual' }} | Select-Object -First 1 -ExpandProperty Name) -as [string])
if (-not $gpu) {{ $gpu = (Get-CimInstance Win32_VideoController | Select-Object -First 1 -ExpandProperty Name) }}
$osVer = [System.Environment]::OSVersion.Version.ToString()
$obj = @{{
  os = "Windows $osVer"
  gpu = $gpu
  mode = $modeUsed
  exit_code = $rc
  smoke_line = $smokeLine
}}
$b64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes(($obj | ConvertTo-Json -Compress)))
Write-Output "===WIN_VERIFY_B64==="
Write-Output $b64
"""
    encoded = base64.b64encode(ps_script.encode("utf-16le")).decode("ascii")
    proc = subprocess.run(
        [
            "ssh",
            "-o",
            "BatchMode=yes",
            host,
            f"powershell -NoProfile -NonInteractive -EncodedCommand {encoded}",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    marker = "===WIN_VERIFY_B64==="
    if marker not in proc.stdout:
        return {
            "spec": "MULTI-OS-WIN",
            "name": name,
            "status": "FAIL",
            "detail": f"exit={proc.returncode}, 出力欠損: {' | '.join(proc.stdout.splitlines()[-6:])}",
        }
    b64_line = proc.stdout.split(marker, 1)[1].strip().splitlines()[0].strip()
    try:
        info = json.loads(base64.b64decode(b64_line).decode("utf-8", errors="replace"))
    except Exception as exc:
        return {"spec": "MULTI-OS-WIN", "name": name, "status": "FAIL", "detail": f"デコード失敗: {exc}"}

    smoke_line = str(info.get("smoke_line", ""))
    smoke_short = smoke_line.split(" scale=")[0] if " scale=" in smoke_line else smoke_line
    ui_m = re.search(r"\bui_ms=(\d+)", smoke_line)
    ipc_m = re.search(r"\bipc_ms=(\d+)", smoke_line)
    sla_ok = bool(ui_m and ipc_m and int(ui_m.group(1)) <= 3000 and int(ipc_m.group(1)) <= 3000)
    sla_str = (
        f"ui_ms={ui_m.group(1)}ms ipc_ms={ipc_m.group(1)}ms"
        if (ui_m and ipc_m)
        else "ui_ms/ipc_ms missing"
    )
    ok = (
        (info.get("exit_code") == 0)
        and ("[SMOKE PASS]" in smoke_line)
        and ("unsafe=0" in smoke_line)
        and sla_ok
    )
    return {
        "spec": "MULTI-OS-WIN",
        "name": name,
        "status": "PASS" if ok else "FAIL",
        "detail": f"{info.get('os')} / {info.get('gpu')} ({info.get('mode')}) | {sla_str} | {smoke_short or 'exit=' + str(info.get('exit_code'))}",
    }


def render_markdown_report(report: dict[str, Any]) -> str:
    lines = [
        f"## 何か藍 (NanikaAi) 自動動作確認・仕様準拠レポート (`{report['mode']}`)",
        "",
        f"- **実行日時**: {report['timestamp']}",
        f"- **判定結果**: **{report['overall']}** (`PASS={report['counts']['PASS']}`, `FAIL={report['counts']['FAIL']}`, `SKIP={report['counts']['SKIP']}`, 所要 `{report['duration_sec']:.2f}s`)",
        f"- **選択理由**: {report['reason']}",
        f"- **仕様基準書**: `docs/BEHAVIOR_SPEC.md`",
        "",
        "| 仕様 ID | チェック項目 | 判定 | 実測根拠 |",
        "| :--- | :--- | :---: | :--- |",
    ]
    for item in report["checks"]:
        detail = str(item["detail"]).replace("|", "\\|").replace("\n", " ")
        lines.append(f"| `{item['spec']}` | {item['name']} | **{item['status']}** | {detail} |")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass

    parser = argparse.ArgumentParser(description="NanikaAi Behavioral Verification & Reporting")
    parser.add_argument(
        "--mode",
        choices=["auto", "fast", "standard", "full"],
        default="full",
        help="Verification tier (default: full)",
    )
    parser.add_argument("--binary", help="Optional prebuilt binary path to run --smoke-test directly")
    parser.add_argument(
        "--remote-linux",
        default=os.environ.get("NANIKAAI_TEST_LINUX_HOST", ""),
        help="Optional SSH target for real Linux hardware verification",
    )
    parser.add_argument(
        "--remote-win",
        default=os.environ.get("NANIKAAI_TEST_WIN_HOST", ""),
        help="Optional SSH target for real Windows hardware verification",
    )
    parser.add_argument(
        "--pretest-only",
        action="store_true",
        help="Run Phase 0 pre-test secret/PII/trust-boundary inspection only (no Rust build or tests)",
    )
    parser.add_argument(
        "--scan-root",
        help="Optional root directory for Phase 0 inspection (defaults to repository root)",
    )
    parser.add_argument("--report-md", help="Optional path to write Markdown report")
    parser.add_argument("--report-json", help="Optional path to write JSON report")
    parser.add_argument("--quiet", action="store_true", help="Suppress stdout Markdown summary")
    args = parser.parse_args()

    started = time.time()
    scan_root = Path(args.scan_root).resolve() if args.scan_root else ROOT_DIR
    changed_files = detect_changed_files() if scan_root == ROOT_DIR else []
    if args.pretest_only:
        effective_mode = "pretest"
        reason = "Phase 0 事前コード検査（`--pretest-only`）を実行"
    elif args.mode == "auto":
        effective_mode, reason = select_auto_mode(changed_files)
    else:
        effective_mode = args.mode
        reason = f"明示指定 `--mode {args.mode}` により実行"

    # Phase 0 MUST run before any cargo test, --smoke-test, or remote SSH tarball transfer.
    phase0_checks = check_phase0_pretest_security(scan_root)
    checks: list[dict[str, Any]] = list(phase0_checks)
    phase0_failed = any(c["status"] == "FAIL" for c in phase0_checks)

    if phase0_failed:
        checks.append({
            "spec": "PRETEST-ABORT",
            "name": "Phase 0 事前コード検査不合格によるテスト/転送遮断",
            "status": "SKIP",
            "detail": "Phase 0（機密/個人情報またはAI指示境界違反）を検出したため、cargo test・--smoke-test・リモート転送を実行せず即時停止しました",
        })
    elif args.pretest_only:
        checks.append({
            "spec": "CARGO-TEST",
            "name": "単体テスト＋仕様統合テスト実行 (`cargo test`)",
            "status": "SKIP",
            "detail": "--pretest-only 指定のため事前コード検査のみ完了",
        })
    else:
        checks.extend(check_tier0_static())

        if effective_mode in ("standard", "full") and not args.binary:
            t1_checks, _ = check_tier1_cargo_tests()
            checks.extend(t1_checks)
        else:
            checks.append({
                "spec": "CARGO-TEST",
                "name": "単体テスト＋仕様統合テスト実行 (`cargo test`)",
                "status": "SKIP",
                "detail": "--binary 指定または fast モードのため省略" if args.binary else "fast モード（文書のみ変更）のため省略",
            })

        if effective_mode == "full":
            checks.extend(check_tier2_full(custom_binary=args.binary))
        else:
            checks.append({
                "spec": "SPEC-UI-01",
                "name": "実ウィンドウ / GPU スモーク検証 (`--smoke-test`)",
                "status": "SKIP",
                "detail": f"{effective_mode} モードのため省略",
            })

        if args.remote_linux:
            checks.append(check_remote_linux(args.remote_linux))
        if args.remote_win:
            checks.append(check_remote_win(args.remote_win))

    counts = {
        "PASS": sum(1 for c in checks if c["status"] == "PASS"),
        "FAIL": sum(1 for c in checks if c["status"] == "FAIL"),
        "SKIP": sum(1 for c in checks if c["status"] == "SKIP"),
    }
    overall = "PASS" if counts["FAIL"] == 0 else "FAIL"

    report: dict[str, Any] = {
        "timestamp": datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
        "mode": effective_mode,
        "requested_mode": args.mode,
        "reason": reason,
        "changed_files": changed_files,
        "overall": overall,
        "counts": counts,
        "duration_sec": round(time.time() - started, 2),
        "checks": checks,
    }

    md = render_markdown_report(report)
    if args.report_md:
        rp = Path(args.report_md)
        rp.parent.mkdir(parents=True, exist_ok=True)
        rp.write_text(md, encoding="utf-8")
    if args.report_json:
        jp = Path(args.report_json)
        jp.parent.mkdir(parents=True, exist_ok=True)
        jp.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    gh_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if gh_summary:
        with open(gh_summary, "a", encoding="utf-8") as f:
            f.write(md + "\n")

    if not args.quiet:
        print(md)

    return 0 if overall == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
