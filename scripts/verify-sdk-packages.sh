#!/bin/sh
set -eu

SDK_PACKAGE_REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SDK_PACKAGE_TMP=$(mktemp -d)
SDK_PACKAGE_CHECK_TARGET="$SDK_PACKAGE_REPO/target/sdk-package-check"
trap 'rm -rf -- "$SDK_PACKAGE_TMP"' EXIT HUP INT TERM

cd "$SDK_PACKAGE_REPO"
SDK_PACKAGE_ID=$(cargo pkgid -p psychevo-ai)
SDK_PACKAGE_VERSION=${SDK_PACKAGE_ID##*#}
case "$SDK_PACKAGE_VERSION" in
  ""|*/*) echo "invalid Psychevo SDK package version: $SDK_PACKAGE_VERSION" >&2; exit 1 ;;
esac

cargo package -p psychevo-ai --allow-dirty
tar -xzf "target/package/psychevo-ai-$SDK_PACKAGE_VERSION.crate" -C "$SDK_PACKAGE_TMP"

cargo package -p psychevo-agent-core \
  --allow-dirty \
  --no-verify \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_TMP/psychevo-ai-$SDK_PACKAGE_VERSION'"
tar -xzf "target/package/psychevo-agent-core-$SDK_PACKAGE_VERSION.crate" -C "$SDK_PACKAGE_TMP"
cargo check \
  --manifest-path "$SDK_PACKAGE_TMP/psychevo-agent-core-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_TMP/psychevo-ai-$SDK_PACKAGE_VERSION'"

cargo package -p psychevo \
  --allow-dirty \
  --no-verify \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_TMP/psychevo-ai-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-agent-core.path='$SDK_PACKAGE_TMP/psychevo-agent-core-$SDK_PACKAGE_VERSION'"
tar -xzf "target/package/psychevo-$SDK_PACKAGE_VERSION.crate" -C "$SDK_PACKAGE_TMP"
cargo check \
  --manifest-path "$SDK_PACKAGE_TMP/psychevo-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --no-default-features \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_TMP/psychevo-ai-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-agent-core.path='$SDK_PACKAGE_TMP/psychevo-agent-core-$SDK_PACKAGE_VERSION'"
