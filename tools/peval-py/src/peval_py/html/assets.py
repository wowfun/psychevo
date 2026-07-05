from __future__ import annotations

from html import escape
from importlib.resources import files

ASSET_PACKAGE = "peval_py.assets"
ECHARTS_VERSION = "6.0.0"
ECHARTS_LOCAL_SRC = f"/assets/echarts/{ECHARTS_VERSION}/echarts.min.js"
ECHARTS_CDN_SRC = f"https://cdn.jsdelivr.net/npm/echarts@{ECHARTS_VERSION}/dist/echarts.min.js"
ASSET_BUNDLES = {
    "report.css": [
        "report_css/00-base.css",
        "report_css/05-data-table.css",
        "report_css/06-leaderboard-summary.css",
        "report_css/08-trajectory.css",
        "report_css/10-trace.css",
        "report_css/12-steps.css",
        "report_css/14-analysis.css",
        "report_css/16-timeline.css",
        "report_css/20-serve-toolbar.css",
        "report_css/22-source-forms.css",
        "report_css/24-source-list-export.css",
        "report_css/26-step-drawer.css",
    ],
    "report.js": [
        "report_js/00-runtime.js",
        "report_js/05-data-tables.js",
        "report_js/06-leaderboard-summary.js",
        "report_js/08-export.js",
        "report_js/09-source-state-controls.js",
        "report_js/10-trajectory-trace.js",
        "report_js/12-serve-controls.js",
        "report_js/14-serve-sources.js",
        "report_js/16-serve-mutations.js",
        "report_js/20-analysis-metrics.js",
        "report_js/22-analysis-notes.js",
        "report_js/24-analysis-rendering.js",
        "report_js/26-analysis-selected.js",
        "report_js/30-timeline-shell.js",
        "report_js/32-timeline-model.js",
        "report_js/34-timeline-chart.js",
        "report_js/36-timeline-table.js",
        "report_js/38-steps.js",
        "report_js/40-markdown.js",
        "report_js/99-entrypoint.js",
    ],
}

def render_echarts_script(mode: str) -> str:
    cdn = escape(ECHARTS_CDN_SRC)
    if mode == "serve":
        local = escape(ECHARTS_LOCAL_SRC)
        return (
            f'<script src="{local}" '
            f'onerror="this.onerror=null;this.src=\'{cdn}\'"></script>'
        )
    return f'<script src="{cdn}"></script>'


def load_asset_text(name: str) -> str:
    if name in ASSET_BUNDLES:
        return "\n".join(load_asset_text(part) for part in ASSET_BUNDLES[name])
    return files(ASSET_PACKAGE).joinpath(name).read_text(encoding="utf-8")


def replace_template_tokens(template: str, values: dict[str, str]) -> str:
    rendered = template
    for key, value in values.items():
        rendered = rendered.replace(f"__{key}__", value)
    return rendered
