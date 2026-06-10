from __future__ import annotations

import json
import math
from html import escape
from importlib import import_module
from importlib.resources import files
from typing import Any

from peval_py.i18n import messages_for, normalize_locale


ASSET_PACKAGE = "peval_py.assets"


def render_html(
    report: dict[str, Any],
    locale: str = "en",
    mode: str = "report",
) -> str:
    normalized_mode = normalize_render_mode(mode)
    normalized_locale = normalize_locale(locale)
    messages = messages_for(normalized_locale)
    payload = HTML_TEMPLATE.replace("__LANG__", escape(normalized_locale))
    payload = payload.replace("__TITLE__", escape(messages["title"]))
    payload = payload.replace("__BODY_CLASS__", escape(f"{normalized_mode}-mode"))
    payload = payload.replace(
        "__SERVE_IMPORT__",
        render_serve_import(report, messages) if normalized_mode == "serve" else "",
    )
    payload = payload.replace("__CSS__", load_asset_text("report.css"))
    payload = payload.replace("__JS__", load_asset_text("report.js"))
    payload = payload.replace(
        "__DATA__",
        safe_json_for_script(json.dumps(report, ensure_ascii=False)),
    )
    payload = payload.replace(
        "__TOKEN_ESTIMATES__",
        safe_json_for_script(
            json.dumps(step_token_estimates(report), ensure_ascii=False)
        ),
    )
    payload = payload.replace(
        "__I18N__",
        safe_json_for_script(json.dumps(messages, ensure_ascii=False)),
    )
    payload = payload.replace(
        "__RENDER_OPTIONS__",
        safe_json_for_script(
            json.dumps(
                {
                    "mode": normalized_mode,
                    "sources": serve_sources(report) if normalized_mode == "serve" else [],
                },
                ensure_ascii=False,
            )
        ),
    )
    return payload


def render_serve_html(report: dict[str, Any], locale: str = "en") -> str:
    return render_html(report, locale=locale, mode="serve")


def normalize_render_mode(mode: object) -> str:
    text = str(mode or "report").strip().lower()
    if text in {"report", "serve"}:
        return text
    raise ValueError(f"unsupported HTML render mode: {mode}; supported modes: report, serve")


def render_serve_import(report: dict[str, Any], messages: dict[str, str]) -> str:
    sources = serve_sources(report)
    count = len(sources)
    source_word = messages["serve_source_count"]
    if count != 1:
        source_word = messages["serve_sources_count"]
    source_list = "".join(
        f'<li><span>{escape(source["label"])}</span><strong>{escape(source["kind"])}</strong></li>'
        for source in sources
    )
    if not source_list:
        source_list = f'<li><span>{escape(messages["serve_no_sources"])}</span><strong>-</strong></li>'
    return f"""
  <section class="serve-import-panel" data-serve-only>
    <details class="serve-import">
      <summary>
        <span class="serve-add">{escape(messages["serve_add_source"])}</span>
        <span class="serve-summary">{count} {escape(source_word)}</span>
      </summary>
      <div class="serve-import-body">
        <div class="serve-drop">{escape(messages["serve_drop_copy"])}</div>
        <ul class="serve-source-list">{source_list}</ul>
      </div>
    </details>
  </section>"""


def serve_sources(report: dict[str, Any]) -> list[dict[str, str]]:
    sources: list[dict[str, str]] = []
    for index, meta in enumerate(list_value(report.get("trajectory_meta"))):
        if not isinstance(meta, dict):
            continue
        data_ref = as_dict(meta.get("data_ref"))
        label = (
            data_ref.get("relative_path")
            or data_ref.get("path")
            or meta.get("trial_key")
            or f"source-{index + 1}"
        )
        kind = meta.get("adapter") or "source"
        sources.append({"label": str(label), "kind": str(kind)})
    return sources


def load_asset_text(name: str) -> str:
    return files(ASSET_PACKAGE).joinpath(name).read_text(encoding="utf-8")


def step_token_estimates(report: dict[str, Any]) -> dict[str, dict[str, dict[str, Any]]]:
    estimates: dict[str, dict[str, dict[str, Any]]] = {}
    trajectories = list_value(report.get("trajectory"))
    metas = list_value(report.get("trajectory_meta"))
    for index, trajectory in enumerate(trajectories):
        if not isinstance(trajectory, dict):
            continue
        meta = metas[index] if index < len(metas) and isinstance(metas[index], dict) else {}
        trial_key = str(
            meta.get("trial_key")
            or trajectory.get("trajectory_id")
            or trajectory.get("session_id")
            or f"input-{index + 1}"
        )
        model = as_dict(trajectory.get("agent")).get("model_name")
        counter, method = token_counter_for_model(model)
        step_estimates: dict[str, dict[str, Any]] = {}
        for step in list_value(trajectory.get("steps")):
            if not isinstance(step, dict):
                continue
            step_id = step.get("step_id")
            if step_id is None or exact_step_token_total(step) is not None:
                continue
            text = visible_step_text(step)
            if not text:
                continue
            tokens = counter(text) if counter else byte_length_token_estimate(text)
            step_estimates[str(step_id)] = {
                "tokens": max(0, int(tokens)),
                "estimated": True,
                "method": method,
                "source": "visible_step_text",
            }
        if step_estimates:
            estimates[trial_key] = step_estimates
    return estimates


def token_counter_for_model(model: object) -> tuple[Any | None, str]:
    try:
        tiktoken = import_module("tiktoken")
    except Exception:  # noqa: BLE001 - optional renderer capability.
        return None, "byte_length_div_4"
    try:
        encoding = tiktoken.encoding_for_model(str(model)) if model else None
    except Exception:  # noqa: BLE001 - model may be unknown to tiktoken.
        encoding = None
    if encoding is None:
        try:
            encoding = tiktoken.get_encoding("o200k_base")
        except Exception:  # noqa: BLE001 - optional renderer capability.
            return None, "byte_length_div_4"
    name = str(getattr(encoding, "name", "") or model or "o200k_base")
    return lambda text: len(encoding.encode(text)), f"tiktoken:{name}"


def byte_length_token_estimate(text: str) -> int:
    return int(math.ceil(len(text.encode("utf-8")) / 4))


def visible_step_text(step: dict[str, Any]) -> str:
    parts: list[str] = []
    append_visible(parts, step.get("reasoning_content"))
    append_visible(parts, step.get("message"))
    for tool in list_value(step.get("tool_calls")):
        if not isinstance(tool, dict):
            continue
        append_visible(parts, tool.get("function_name"))
        append_visible(parts, tool.get("arguments"))
    for observation in list_value(as_dict(step.get("observation")).get("results")):
        if not isinstance(observation, dict):
            continue
        append_visible(parts, observation.get("content"))
    return "\n".join(part for part in parts if part)


def append_visible(parts: list[str], value: Any) -> None:
    text = visible_value(value)
    if text:
        parts.append(text)


def visible_value(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value.strip()
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


def exact_step_token_total(step: dict[str, Any]) -> int | None:
    metrics = as_dict(step.get("metrics"))
    usage = as_dict(metrics.get("usage"))
    values = [
        numeric_value(metrics.get("prompt_tokens")),
        numeric_value(metrics.get("completion_tokens")),
        numeric_value(metrics.get("cached_tokens")),
        numeric_value(usage.get("total_tokens")),
    ]
    present = [value for value in values if value is not None]
    return int(sum(present)) if present else None


def numeric_value(value: Any) -> float | None:
    if value is None or value == "":
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if math.isnan(number):
        return None
    return number


def as_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def list_value(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []


HTML_TEMPLATE = """<!doctype html>
<html lang="__LANG__">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>__TITLE__</title>
<style>
__CSS__
</style>
</head>
<body class="__BODY_CLASS__">
<div class="workspace">
  __SERVE_IMPORT__
  <section class="topline">
    <h1>__TITLE__</h1>
  </section>
  <section id="report-notes"></section>
  <section class="panel-stack" id="comparison"></section>
  <section class="trace-panel" id="trace"></section>
</div>
<aside class="step-drawer" id="step-drawer" hidden></aside>
<script type="application/json" id="peval-py-data">__DATA__</script>
<script type="application/json" id="peval-py-token-estimates">__TOKEN_ESTIMATES__</script>
<script type="application/json" id="peval-py-i18n">__I18N__</script>
<script type="application/json" id="peval-py-render-options">__RENDER_OPTIONS__</script>
<script src="https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js"></script>
<script>
__JS__
</script>
</body>
</html>"""


def safe_json_for_script(value: str) -> str:
    return value.replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e")
