#!/bin/sh
set -eu

SDK_PACKAGE_REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SDK_PACKAGE_TMP=$(mktemp -d)
SDK_PACKAGE_WORKSPACE="$SDK_PACKAGE_TMP/workspace"
SDK_PACKAGE_EXTRACT="$SDK_PACKAGE_TMP/extracted"
SDK_PACKAGE_CHECK_TARGET="$SDK_PACKAGE_REPO/target/sdk-package-check"
trap 'rm -rf -- "$SDK_PACKAGE_TMP"' EXIT HUP INT TERM

cd "$SDK_PACKAGE_REPO"
SDK_PACKAGE_ID=$(cargo pkgid -p psychevo-ai)
SDK_PACKAGE_VERSION=${SDK_PACKAGE_ID##*#}
case "$SDK_PACKAGE_VERSION" in
  ""|*/*) echo "invalid Psychevo SDK package version: $SDK_PACKAGE_VERSION" >&2; exit 1 ;;
esac

mkdir -p "$SDK_PACKAGE_WORKSPACE" "$SDK_PACKAGE_EXTRACT"
tar -cf - Cargo.toml Cargo.lock .cargo crates xtask README.md LICENSE |
  (cd "$SDK_PACKAGE_WORKSPACE" && tar -xf -)

cd "$SDK_PACKAGE_WORKSPACE"
cargo package \
  -p psychevo-ai \
  --allow-dirty \
  --target-dir "$SDK_PACKAGE_REPO/target"
tar -xzf "$SDK_PACKAGE_REPO/target/package/psychevo-ai-$SDK_PACKAGE_VERSION.crate" \
  -C "$SDK_PACKAGE_EXTRACT"
cargo check \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --all-targets \
  --no-default-features
cargo check \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --all-targets \
  --all-features
cargo test \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --doc \
  --all-features

cargo package -p psychevo-agent-core \
  --allow-dirty \
  --no-verify \
  --target-dir "$SDK_PACKAGE_REPO/target" \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'"
tar -xzf "$SDK_PACKAGE_REPO/target/package/psychevo-agent-core-$SDK_PACKAGE_VERSION.crate" \
  -C "$SDK_PACKAGE_EXTRACT"
cargo check \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-agent-core-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --all-targets \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'"
cargo test \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-agent-core-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --doc \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'"

cargo package -p psychevo-extension-protocol \
  --allow-dirty \
  --target-dir "$SDK_PACKAGE_REPO/target"
tar -xzf "$SDK_PACKAGE_REPO/target/package/psychevo-extension-protocol-$SDK_PACKAGE_VERSION.crate" \
  -C "$SDK_PACKAGE_EXTRACT"
cargo check \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-extension-protocol-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --all-targets
cargo test \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-extension-protocol-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --doc

cargo package -p psychevo \
  --allow-dirty \
  --no-verify \
  --target-dir "$SDK_PACKAGE_REPO/target" \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-agent-core.path='$SDK_PACKAGE_EXTRACT/psychevo-agent-core-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-extension-protocol.path='$SDK_PACKAGE_EXTRACT/psychevo-extension-protocol-$SDK_PACKAGE_VERSION'"
tar -xzf "$SDK_PACKAGE_REPO/target/package/psychevo-$SDK_PACKAGE_VERSION.crate" \
  -C "$SDK_PACKAGE_EXTRACT"
cargo check \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --all-targets \
  --no-default-features \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-agent-core.path='$SDK_PACKAGE_EXTRACT/psychevo-agent-core-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-extension-protocol.path='$SDK_PACKAGE_EXTRACT/psychevo-extension-protocol-$SDK_PACKAGE_VERSION'"
cargo test \
  --manifest-path "$SDK_PACKAGE_EXTRACT/psychevo-$SDK_PACKAGE_VERSION/Cargo.toml" \
  --target-dir "$SDK_PACKAGE_CHECK_TARGET" \
  --doc \
  --no-default-features \
  --config "patch.crates-io.psychevo-ai.path='$SDK_PACKAGE_EXTRACT/psychevo-ai-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-agent-core.path='$SDK_PACKAGE_EXTRACT/psychevo-agent-core-$SDK_PACKAGE_VERSION'" \
  --config "patch.crates-io.psychevo-extension-protocol.path='$SDK_PACKAGE_EXTRACT/psychevo-extension-protocol-$SDK_PACKAGE_VERSION'"
