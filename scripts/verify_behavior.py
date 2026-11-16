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
    smoke_short = smoke_det.split(" scale=")[0] if " scale=" in smoke_det else smoke_det
    ok = (run_proc.returncode == 0) and (rep.get("overall") == "PASS")
    return {
        "spec": "MULTI-OS-LINUX",
        "name": name,
        "status": "PASS" if ok else "FAIL",
        "detail": f"{rep.get('counts')} | {cargo_det} | {smoke_short}",
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
    ok = (info.get("exit_code") == 0) and ("[SMOKE PASS]" in smoke_line) and ("unsafe=0" in smoke_line)
    return {
        "spec": "MULTI-OS-WIN",
        "name": name,
        "status": "PASS" if ok else "FAIL",
        "detail": f"{info.get('os')} / {info.get('gpu')} ({info.get('mode')}) | {smoke_short or 'exit=' + str(info.get('exit_code'))}",
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
    parser.add_argument("--report-md", help="Optional path to write Markdown report")
    parser.add_argument("--report-json", help="Optional path to write JSON report")
    parser.add_argument("--quiet", action="store_true", help="Suppress stdout Markdown summary")
    args = parser.parse_args()

    started = time.time()
    changed_files = detect_changed_files()
    if args.mode == "auto":
        effective_mode, reason = select_auto_mode(changed_files)
    else:
        effective_mode = args.mode
        reason = f"明示指定 `--mode {args.mode}` により実行"

    checks: list[dict[str, Any]] = []
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
