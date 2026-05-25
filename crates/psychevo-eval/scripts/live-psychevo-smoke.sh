#!/usr/bin/env sh
set -eu

PROJECT="${1:-crates/psychevo-eval/fixtures/local-coding}"
SUITE="${PEVAL_LIVE_SUITE:-rust-swe}"
AGENT="${PEVAL_LIVE_AGENT:-psychevo-live}"
RUN_ID="${PEVAL_LIVE_RUN_ID:-live-psychevo-smoke}"

MANIFEST="$PROJECT/eval.toml"
if [ ! -f "$MANIFEST" ]; then
    echo "missing eval.toml: $MANIFEST" >&2
    exit 2
fi

if ! grep -Eq '^[[:space:]]*allow_live[[:space:]]*=[[:space:]]*true([[:space:]]*(#.*)?)?$' "$MANIFEST"; then
    echo "refusing live Psychevo smoke: $MANIFEST must set allow_live = true" >&2
    exit 2
fi

REAL_HOME="${PSYCHEVO_HOME:-$HOME/.psychevo}"
if [ -z "${PSYCHEVO_CONFIG:-}" ] && [ -f "$REAL_HOME/config.toml" ]; then
    PSYCHEVO_CONFIG="$REAL_HOME/config.toml"
    export PSYCHEVO_CONFIG
fi

LIVE_HOME="$(mktemp -d "${TMPDIR:-/tmp}/psychevo-peval-live-home.XXXXXX")"
trap 'rm -rf "$LIVE_HOME"' EXIT HUP INT TERM
PSYCHEVO_HOME="$LIVE_HOME"
PSYCHEVO_DB="$LIVE_HOME/state.db"
PEVAL_ROOT="${PEVAL_ROOT:-$(pwd)/.local/evals}"
export PSYCHEVO_HOME PSYCHEVO_DB PEVAL_ROOT

cargo build -p psychevo-cli --bin pevo
PATH="$(pwd)/target/debug:$PATH"
export PATH

cargo run -p psychevo-eval --bin peval -- run \
    --config "$MANIFEST" \
    --suite "$SUITE" \
    --agent "$AGENT" \
    --run-id "$RUN_ID"
