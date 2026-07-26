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
    candidates = [ROOT / "target" / "release" / f"pevo{suffix}"]
    candidates.extend((package_root / "python").glob("wheels/*.whl"))
    candidates.extend((package_root / "python").glob("sdists/*"))
    artifacts = sorted(path for path in candidates if path.is_file())
    if not artifacts:
        raise RuntimeError("no package artifacts found for checksumming")
    rows = [
        f"{digest(path)}  {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}"
        for path in artifacts
    ]
    output = package_root / "checksums.sha256"
    output.write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"package checksums: {len(rows)} artifact(s) ({output})")


if __name__ == "__main__":
    main()
