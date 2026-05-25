set -eu

if cargo test --quiet >peval-score.stdout 2>peval-score.stderr; then
    cat <<'JSON'
{"schema_version":1,"passed":true,"score":1.0,"message":"cargo test passed","details":{"scorer":"cargo test"}}
JSON
else
    cat peval-score.stderr >&2 || true
    cat <<'JSON'
{"schema_version":1,"passed":false,"score":0.0,"message":"cargo test failed","details":{"scorer":"cargo test"}}
JSON
fi
