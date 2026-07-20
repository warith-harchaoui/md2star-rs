#!/usr/bin/env bash
# brew.sh — one-shot dev bootstrap for md2star-rs on macOS / Linux via Homebrew.
#
# Installs a Rust toolchain (rustup → stable, with rustfmt + clippy) and builds the
# release binary. Idempotent: re-running only fills in what's missing. Windows users
# should instead grab rustup from https://rustup.rs and run `cargo build --release`.
set -euo pipefail

# Resolve the repo root from this script's location so it works from any cwd.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# 1. Ensure Homebrew is present (it provides rustup-init on both macOS and Linux).
if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew not found. Install it from https://brew.sh first." >&2
  exit 1
fi

# 2. Ensure a Rust toolchain. Prefer an existing cargo; otherwise install via rustup.
if ! command -v cargo >/dev/null 2>&1; then
  echo "Installing Rust via rustup (Homebrew)…"
  brew install rustup
  rustup-init -y --default-toolchain stable
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

# 3. Make sure the components CI enforces are available locally too.
rustup component add rustfmt clippy >/dev/null 2>&1 || true

# 4. Build the release binary so `target/release/md2docx` is ready to run.
echo "Building md2star-rs (release)…"
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

echo "Done. Binary: $REPO_ROOT/target/release/md2docx"
echo "Try:  $REPO_ROOT/target/release/md2docx --help"
