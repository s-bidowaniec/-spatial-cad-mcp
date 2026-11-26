#!/usr/bin/env bash
# Run the full local gate - the same checks CI runs on every push and PR.
# Usage: bash tools/check_all.sh
set -euo pipefail
cd "$(dirname "$0")/.."

uv sync --locked --all-packages
uv run ruff check .
uv run ruff format --check .
uv run pytest
uv run python tools/check_docs.py
uv run python tools/check_no_data.py
uv run python tools/check_tools.py
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

echo "ALL CHECKS PASSED"
