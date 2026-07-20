#!/bin/sh
set -eu

test "$#" -eq 5
test "$1" = ci
test "$2" = --omit=dev
test "$3" = --ignore-scripts
test "$4" = --no-audit
test "$5" = --no-fund
test -n "${PSYCHEVO_TEST_CODEX_ACP_PACKAGE:-}"
test -n "${PSYCHEVO_TEST_CODEX_ACP_VERSION:-}"
if test -f "$0.require-captured-env"; then
  test "${PSYCHEVO_CAPTURED_INSTALL_ENV:-}" = captured
  test -z "${HOME+x}"
fi

mkdir -p node_modules/@agentclientprotocol/codex-acp/dist node_modules/.bin
printf '{"name":"%s","version":"%s"}' \
  "$PSYCHEVO_TEST_CODEX_ACP_PACKAGE" \
  "$PSYCHEVO_TEST_CODEX_ACP_VERSION" \
  > node_modules/@agentclientprotocol/codex-acp/package.json
printf '%s\n' '#!/bin/sh' 'exit 0' > node_modules/@agentclientprotocol/codex-acp/dist/cli.js
chmod 755 node_modules/@agentclientprotocol/codex-acp/dist/cli.js
ln -s ../@agentclientprotocol/codex-acp/dist/cli.js node_modules/.bin/codex-acp
