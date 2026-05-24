set -eu

workspace="$1"
prompt="$2"
timeout_seconds="${PEVAL_LIVE_PEVO_TIMEOUT_SECONDS:-75}"
stdout_log="$workspace/pevo-live.stdout"
stderr_log="$workspace/pevo-live.stderr"
postcheck_stdout="$workspace/pevo-live-postcheck.stdout"
postcheck_stderr="$workspace/pevo-live-postcheck.stderr"

rm -f "$stdout_log" "$stderr_log" "$postcheck_stdout" "$postcheck_stderr"

set +e
timeout -k 5 "$timeout_seconds" pevo run \
    --dir "$workspace" \
    --format json \
    --variant none \
    --dangerously-skip-permissions \
    --no-skills \
    --no-agents \
    "$prompt" >"$stdout_log" 2>"$stderr_log"
pevo_status=$?
set -e

if [ "$pevo_status" -eq 0 ]; then
    echo "pevo exited successfully" >&2
    exit 0
fi

if cargo test --quiet >"$postcheck_stdout" 2>"$postcheck_stderr"; then
    echo "pevo exited with status $pevo_status, but post-run cargo test passed" >&2
    echo "--- pevo stdout tail ---" >&2
    tail -80 "$stdout_log" >&2 || true
    echo "--- pevo stderr tail ---" >&2
    tail -80 "$stderr_log" >&2 || true
    exit 0
fi

echo "--- pevo stdout tail ---" >&2
tail -80 "$stdout_log" >&2 || true
echo "--- pevo stderr tail ---" >&2
tail -80 "$stderr_log" >&2 || true
cat "$postcheck_stderr" >&2 || true
exit "$pevo_status"
