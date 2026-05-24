import argparse
import json
import re
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(
        description="Render the deterministic pevo TUI VHS tape template."
    )
    parser.add_argument("--template", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--psychevo-home", required=True)
    parser.add_argument("--psychevo-db", required=True)
    parser.add_argument("--psychevo-config", required=True)
    parser.add_argument("--pevo-cmd", required=True)
    args = parser.parse_args()

    replacements = {
        "{{PSYCHEVO_HOME}}": json.dumps(args.psychevo_home),
        "{{PSYCHEVO_DB}}": json.dumps(args.psychevo_db),
        "{{PSYCHEVO_CONFIG}}": json.dumps(args.psychevo_config),
        "{{PEVO_CMD}}": json.dumps(args.pevo_cmd),
    }

    text = Path(args.template).read_text(encoding="utf-8")
    for placeholder, value in replacements.items():
        text = text.replace(placeholder, value)

    unresolved = sorted(set(re.findall(r"\{\{[A-Z0-9_]+\}\}", text)))
    if unresolved:
        raise SystemExit(f"unresolved tape placeholder(s): {', '.join(unresolved)}")

    Path(args.output).write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
