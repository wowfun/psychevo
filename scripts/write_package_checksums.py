from __future__ import annotations

import hashlib
import os
from pathlib import Path

ROOT = Path(__file__).parents[1]


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def main() -> None:
    artifact_root = Path(
        os.environ.get(
            "PSYCHEVO_CI_ARTIFACT_ROOT",
            ROOT / ".local" / "ci-artifacts" / "package",
        )
    )
    package_root = artifact_root / "package"
    package_root.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if os.name == "nt" else ""
    groups = {
        "CLI": [package_root / "cli-target" / "release" / f"pevo{suffix}"],
        "Python wheels": list((package_root / "python").glob("wheels/*.whl")),
        "Python sdists": list((package_root / "python").glob("sdists/*")),
        "Desktop bundles": [
            path
            for path in (
                package_root / "desktop-target" / "release" / "bundle"
            ).rglob("*")
            if path.is_file()
        ],
    }
    missing = [name for name, paths in groups.items() if not any(path.is_file() for path in paths)]
    if missing:
        raise RuntimeError(f"missing package artifacts: {', '.join(missing)}")
    artifacts = sorted(
        path
        for paths in groups.values()
        for path in paths
        if path.is_file()
    )
    rows = [
        f"{digest(path)}  {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}"
        for path in artifacts
    ]
    output = package_root / "checksums.sha256"
    output.write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"package checksums: {len(rows)} artifact(s) ({output})")


if __name__ == "__main__":
    main()
