from __future__ import annotations

MAX_NOTE_BYTES = 1024 * 1024
PEVAL_PY_CONFIG = "peval-py.toml"
ANALYSIS_JSON_FILENAME = "analysis.json"
ANALYSIS_MD_FILENAME = "analysis.md"
MERGEABLE_ANALYSIS_LIST_FIELDS = (
    "findings",
    "recommendations",
    "limitations",
    "commands",
)
ANALYSIS_REPORT_FIELDS = (
    "summary",
    "analysis_status",
    "subject",
    "findings",
    "recommendations",
    "limitations",
    "commands",
    "analysis_metrics",
    "confidence",
)
ANALYSIS_INPUT_FIELDS = (
    "summary",
    "status",
    "findings",
    "recommendations",
    "limitations",
    "confidence",
)
ANALYSIS_INPUT_EXTRA_FIELD = "extra"
ANALYSIS_IMPORT_HINT_FIELDS = (
    "subject",
    "metrics",
    "commands",
    "analysis_status",
    "analysis_metrics",
    "auto",
)
RESERVED_ANALYSIS_METRIC_KEYS = {"auto"}
