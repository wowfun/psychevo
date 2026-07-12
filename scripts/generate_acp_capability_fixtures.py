#!/usr/bin/env python3
"""Generate reviewed ACP capability-pack fixtures from local source evidence.

This intentionally does not parse TypeScript. Each fixture field is admitted only
after an exact, human-reviewed source marker is present. Package identity and
version are read from package.json and must equal the reviewed release. Source
hashes make otherwise-unnoticed source drift fail ``--check``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any


REVIEWED_CODEX_NAME = "@agentclientprotocol/codex-acp"
REVIEWED_CODEX_VERSION = "1.1.2"
REVIEWED_OPENCODE_NAME = "opencode"
REVIEWED_OPENCODE_AGENT_NAME = "OpenCode"
REVIEWED_OPENCODE_VERSION = "1.17.18"

FIXTURE_DIR = Path("crates/psychevo-gateway/tests/fixtures/acp_capability_packs")


class EvidenceError(RuntimeError):
    """The reviewed source no longer proves the committed fixture."""


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise EvidenceError(f"required reviewed source is missing: {path}") from error


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(read_text(path))
    except json.JSONDecodeError as error:
        raise EvidenceError(f"reviewed package metadata is invalid JSON: {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError(f"reviewed package metadata must be an object: {path}")
    return value


def require_package(
    path: Path,
    *,
    expected_name: str,
    expected_version: str,
) -> dict[str, Any]:
    package = read_json(path)
    actual = (package.get("name"), package.get("version"))
    expected = (expected_name, expected_version)
    if actual != expected:
        raise EvidenceError(
            f"reviewed package identity drifted in {path}: expected {expected!r}, got {actual!r}"
        )
    return package


def require_markers(path: Path, markers: dict[str, str]) -> list[str]:
    source = read_text(path)
    missing = [marker_id for marker_id, marker in markers.items() if marker not in source]
    if missing:
        raise EvidenceError(
            f"reviewed capability markers missing from {path}: {', '.join(missing)}"
        )
    return list(markers)


def sha256(path: Path) -> str:
    try:
        content = path.read_bytes()
    except FileNotFoundError as error:
        raise EvidenceError(f"required reviewed source is missing: {path}") from error
    return hashlib.sha256(content).hexdigest()


def pretty_json(value: Any) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def source_record(repo_root: Path, path: Path, marker_ids: list[str]) -> dict[str, Any]:
    return {
        "path": path.relative_to(repo_root).as_posix(),
        "sha256": sha256(path),
        "assertedMarkers": marker_ids,
    }


def generate(repo_root: Path, references_root: Path) -> dict[Path, bytes]:
    codex_package_path = references_root / "codex-acp/package.json"
    codex_server_path = references_root / "codex-acp/src/CodexAcpServer.ts"
    codex_auth_path = references_root / "codex-acp/src/CodexAuthMethod.ts"
    opencode_package_path = references_root / "opencode/packages/opencode/package.json"
    opencode_service_path = references_root / "opencode/packages/opencode/src/acp/service.ts"
    opencode_version_path = references_root / "opencode/packages/core/src/installation/version.ts"
    opencode_build_path = references_root / "opencode/packages/opencode/script/build.ts"

    codex_package = require_package(
        codex_package_path,
        expected_name=REVIEWED_CODEX_NAME,
        expected_version=REVIEWED_CODEX_VERSION,
    )
    opencode_package = require_package(
        opencode_package_path,
        expected_name=REVIEWED_OPENCODE_NAME,
        expected_version=REVIEWED_OPENCODE_VERSION,
    )
    codex_sdk = codex_package.get("dependencies", {}).get("@agentclientprotocol/sdk")
    if codex_sdk != "^1.2.1":
        raise EvidenceError(
            f"reviewed Codex ACP SDK dependency drifted: expected '^1.2.1', got {codex_sdk!r}"
        )
    opencode_sdk = opencode_package.get("dependencies", {}).get("@agentclientprotocol/sdk")
    if opencode_sdk != "0.21.0":
        raise EvidenceError(
            f"reviewed OpenCode ACP SDK dependency drifted: expected '0.21.0', got {opencode_sdk!r}"
        )

    codex_server_markers = require_markers(
        codex_server_path,
        {
            "identity-from-package": 'import packageJson from "../package.json";',
            "protocol-from-reviewed-sdk": "protocolVersion: acp.PROTOCOL_VERSION,",
            "agent-name-from-package": "name: packageJson.name,",
            "agent-title-codex": 'title: "Codex",',
            "agent-version-from-package": "version: packageJson.version,",
            "auth-logout": "logout: {},",
            "providers": "providers: {},",
            "load-session": "loadSession: true,",
            "prompt-embedded-context": "embeddedContext: true,",
            "prompt-image": "image: true",
            "session-resume": "resume: { },",
            "session-list": "list: { },",
            "session-close": "close: { },",
            "session-delete": "delete: { },",
            "session-additional-directories": "additionalDirectories: {},",
            "mcp-acp-disabled": "acp: false,",
            "mcp-http": "http: true,",
            "mcp-sse-disabled": "sse: false",
            "auth-methods-from-helper": "authMethods: getCodexAuthMethods(_params.clientCapabilities),",
        },
    )
    codex_auth_markers = require_markers(
        codex_auth_path,
        {
            "api-key-id": 'id: "api-key",',
            "api-key-name": 'name: "API Key",',
            "api-key-description": 'description: "Use an API key to authenticate",',
            "api-key-provider": 'provider: "openai"',
            "chat-gpt-id": 'id: "chat-gpt",',
            "chat-gpt-name": 'name: "ChatGPT",',
            "chat-gpt-description": 'description: "Use ChatGPT to authenticate"',
            "browser-auth-condition": 'if (!env["NO_BROWSER"]) {',
            "gateway-id": 'id: "gateway",',
            "gateway-name": 'name: "Custom model gateway",',
            "gateway-description": 'description: "Use a custom gateway to authenticate and access models",',
            "gateway-client-opt-in": 'clientCapabilities?.auth?._meta?.["gateway"] === true',
        },
    )
    opencode_service_markers = require_markers(
        opencode_service_path,
        {
            "version-from-installation": 'import { InstallationVersion } from "@opencode-ai/core/installation/version"',
            "auth-method-id": 'export const AuthMethodID = "opencode-login"',
            "auth-description": 'description: "Run `opencode auth login` in the terminal",',
            "auth-name": 'name: "Login with opencode",',
            "terminal-auth-opt-in": 'params.clientCapabilities?._meta?.["terminal-auth"] === true',
            "terminal-auth-command": 'command: "opencode",',
            "terminal-auth-args": 'args: ["auth", "login"],',
            "terminal-auth-label": 'label: "OpenCode Login",',
            "protocol-v1": "protocolVersion: 1,",
            "load-session": "loadSession: true,",
            "mcp-http": "http: true,",
            "mcp-sse": "sse: true,",
            "prompt-embedded-context": "embeddedContext: true,",
            "prompt-image": "image: true,",
            "session-close": "close: {},",
            "session-fork": "fork: {},",
            "session-list": "list: {},",
            "session-resume": "resume: {},",
            "auth-method-list": "authMethods: [authMethod],",
            "agent-name": 'name: "OpenCode",',
            "agent-version": "version: InstallationVersion,",
        },
    )
    opencode_version_markers = require_markers(
        opencode_version_path,
        {
            "build-version-declaration": "const OPENCODE_VERSION: string",
            "installation-version-from-build": 'typeof OPENCODE_VERSION === "string" ? OPENCODE_VERSION : "local"',
        },
    )
    opencode_build_markers = require_markers(
        opencode_build_path,
        {
            "build-injects-script-version": "OPENCODE_VERSION: `'${Script.version}'`,",
        },
    )

    codex_initialize = {
        "protocolVersion": 1,
        "agentInfo": {
            "name": codex_package["name"],
            "title": "Codex",
            "version": codex_package["version"],
        },
        "agentCapabilities": {
            "auth": {"logout": {}},
            "providers": {},
            "loadSession": True,
            "promptCapabilities": {"embeddedContext": True, "image": True},
            "sessionCapabilities": {
                "resume": {},
                "list": {},
                "close": {},
                "delete": {},
                "additionalDirectories": {},
            },
            "mcpCapabilities": {"acp": False, "http": True, "sse": False},
        },
        # Deterministic reviewed scenario: browser auth is enabled and the
        # client opts into the source's gateway-auth metadata extension.
        "authMethods": [
            {
                "id": "api-key",
                "name": "API Key",
                "description": "Use an API key to authenticate",
                "_meta": {"api-key": {"provider": "openai"}},
            },
            {
                "id": "chat-gpt",
                "name": "ChatGPT",
                "description": "Use ChatGPT to authenticate",
            },
            {
                "id": "gateway",
                "name": "Custom model gateway",
                "description": "Use a custom gateway to authenticate and access models",
                "_meta": {"gateway": {"protocol": "openai", "restartRequired": "false"}},
            },
        ],
    }
    opencode_initialize = {
        "protocolVersion": 1,
        "agentCapabilities": {
            "loadSession": True,
            "mcpCapabilities": {"http": True, "sse": True},
            "promptCapabilities": {"embeddedContext": True, "image": True},
            "sessionCapabilities": {"close": {}, "fork": {}, "list": {}, "resume": {}},
        },
        # Deterministic reviewed scenario: the client opts into terminal-auth.
        "authMethods": [
            {
                "description": "Run `opencode auth login` in the terminal",
                "name": "Login with opencode",
                "id": "opencode-login",
                "_meta": {
                    "terminal-auth": {
                        "command": "opencode",
                        "args": ["auth", "login"],
                        "label": "OpenCode Login",
                    }
                },
            }
        ],
        "agentInfo": {
            "name": REVIEWED_OPENCODE_AGENT_NAME,
            "version": opencode_package["version"],
        },
    }

    codex_fixture_path = FIXTURE_DIR / "codex_initialize_v1.json"
    opencode_fixture_path = FIXTURE_DIR / "opencode_initialize_v1.json"
    manifest_path = FIXTURE_DIR / "source-evidence.json"
    manifest = {
        "schemaVersion": 1,
        "generatedBy": "scripts/generate_acp_capability_fixtures.py",
        "policy": "Exact reviewed package identity plus asserted source markers; source SHA256 detects drift.",
        "fixtures": {
            "codex": {
                "fixture": codex_fixture_path.as_posix(),
                "scenario": "gateway auth opted in; browser auth enabled",
                "packageIdentity": {
                    "name": codex_package["name"],
                    "version": codex_package["version"],
                },
                "sources": [
                    source_record(
                        repo_root,
                        codex_package_path,
                        ["package-name", "package-version", "acp-sdk-dependency"],
                    ),
                    source_record(repo_root, codex_server_path, codex_server_markers),
                    source_record(repo_root, codex_auth_path, codex_auth_markers),
                ],
            },
            "opencode": {
                "fixture": opencode_fixture_path.as_posix(),
                "scenario": "terminal-auth opted in; reviewed release build version",
                "packageIdentity": {
                    "name": opencode_package["name"],
                    "agentName": REVIEWED_OPENCODE_AGENT_NAME,
                    "version": opencode_package["version"],
                },
                "sources": [
                    source_record(
                        repo_root,
                        opencode_package_path,
                        ["package-name", "package-version", "acp-sdk-dependency"],
                    ),
                    source_record(repo_root, opencode_service_path, opencode_service_markers),
                    source_record(repo_root, opencode_version_path, opencode_version_markers),
                    source_record(repo_root, opencode_build_path, opencode_build_markers),
                ],
            },
        },
    }
    return {
        repo_root / codex_fixture_path: pretty_json(codex_initialize),
        repo_root / opencode_fixture_path: pretty_json(opencode_initialize),
        repo_root / manifest_path: pretty_json(manifest),
    }


def write_or_check(outputs: dict[Path, bytes], *, check: bool) -> int:
    stale: list[Path] = []
    for path, expected in outputs.items():
        actual = path.read_bytes() if path.is_file() else None
        if actual == expected:
            continue
        if check:
            stale.append(path)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(expected)
        print(f"wrote {path}")
    if stale:
        for path in stale:
            print(f"stale generated ACP capability fixture: {path}", file=sys.stderr)
        print(
            "run scripts/generate_acp_capability_fixtures.py after reviewing the source drift",
            file=sys.stderr,
        )
        return 1
    if check:
        print("ACP capability fixtures match reviewed local source evidence")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="compare committed fixtures and evidence hashes without writing",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    references_root = repo_root / ".references"
    try:
        outputs = generate(repo_root, references_root)
    except EvidenceError as error:
        print(f"ACP capability fixture evidence check failed: {error}", file=sys.stderr)
        return 1
    return write_or_check(outputs, check=args.check)


if __name__ == "__main__":
    raise SystemExit(main())
