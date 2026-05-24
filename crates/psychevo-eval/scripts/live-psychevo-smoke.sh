#!/usr/bin/env sh
set -eu

PROJECT="${1:-crates/psychevo-eval/fixtures/local-rust-swe}"
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

LIVE_HOME="$PROJECT/target/peval/live-home"
mkdir -p "$LIVE_HOME"
PSYCHEVO_HOME="$LIVE_HOME"
PSYCHEVO_DB="$LIVE_HOME/state.db"
export PSYCHEVO_HOME PSYCHEVO_DB

cargo build -p psychevo-cli --bin pevo
PATH="$(pwd)/target/debug:$PATH"
export PATH

cargo run -p psychevo-eval --bin peval -- run \
    --project "$PROJECT" \
    --suite "$SUITE" \
    --agent "$AGENT" \
    --run-id "$RUN_ID"
