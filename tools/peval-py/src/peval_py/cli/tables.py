from __future__ import annotations

def format_workspace_source_table(sources: list[dict]) -> str:
    headers = [
        "#",
        "source_key",
        "session_id",
        "trial_key",
        "active",
        "kind",
        "adapter",
        "alias/name",
    ]
    rows = [
        [
            str(index),
            value_or_dash(source.get("source_key")),
            value_or_dash(source.get("trial_session_id") or source.get("session_id")),
            value_or_dash(source.get("trial_key")),
            "yes" if source.get("active") else "no",
            value_or_dash(source.get("kind")),
            value_or_dash(source.get("adapter")),
            value_or_dash(source.get("source_alias") or source.get("label")),
        ]
        for index, source in enumerate(sources, start=1)
    ]
    widths = [
        max(len(row[column]) for row in [headers, *rows])
        for column in range(len(headers))
    ]
    lines = [format_table_row(headers, widths)]
    lines.extend(format_table_row(row, widths) for row in rows)
    return "\n".join(lines) + "\n"


def format_table_row(row: list[str], widths: list[int]) -> str:
    return "  ".join(value.ljust(widths[index]) for index, value in enumerate(row))


def value_or_dash(value: object) -> str:
    if value is None:
        return "-"
    text = str(value)
    return text if text else "-"
