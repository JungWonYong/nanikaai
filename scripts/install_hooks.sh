#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

chmod +x "$ROOT_DIR/.githooks/pre-commit" "$ROOT_DIR/.githooks/pre-push" "$ROOT_DIR/scripts/verify_behavior.py"
git config core.hooksPath .githooks
echo "[OK] Git hooks enabled (core.hooksPath=.githooks). pre-commit (--pretest-only) and pre-push (--mode auto) are active."
