from __future__ import annotations

import json
import math
from html import escape
from importlib import import_module
from importlib.resources import files
from typing import Any

from peval_py.adapters import available_adapter_ids
from peval_py.i18n import messages_for, normalize_locale


ASSET_PACKAGE = "peval_py.assets"
ECHARTS_VERSION = "6.0.0"
ECHARTS_LOCAL_SRC = f"/assets/echarts/{ECHARTS_VERSION}/echarts.min.js"
ECHARTS_CDN_SRC = f"https://cdn.jsdelivr.net/npm/echarts@{ECHARTS_VERSION}/dist/echarts.min.js"
ASSET_BUNDLES = {
    "report.css": [
        "report_css/00-shell.css",
        "report_css/10-trace-analysis-timeline.css",
        "report_css/20-serve-source-drawer.css",
    ],
    "report.js": [
        "report_js/00-core.js",
        "report_js/10-trajectory-serve.js",
        "report_js/20-analysis.js",
        "report_js/30-timeline.js",
    ],
}


def render_html(
    report: dict[str, Any],
    locale: str = "en",
    mode: str = "report",
    sources: list[dict[str, Any]] | None = None,
    adapter_defaults: dict[str, str] | None = None,
) -> str:
    normalized_mode = normalize_render_mode(mode)
    normalized_locale = normalize_locale(locale)
    messages = messages_for(normalized_locale)
    serve_source_payload = (
        list(sources) if sources is not None else serve_sources(report)
    )
    render_options: dict[str, Any] = {
        "mode": normalized_mode,
        "sources": serve_source_payload if normalized_mode == "serve" else [],
    }
    if normalized_mode == "serve":
        render_options["adapter_defaults"] = adapter_defaults or {}
    payload = load_asset_text("report.html").replace("__LANG__", escape(normalized_locale))
    payload = payload.replace("__TITLE__", escape(messages["title"]))
    payload = payload.replace("__BODY_CLASS__", escape(f"{normalized_mode}-mode"))
    payload = payload.replace(
        "__SERVE_SOURCE_MANAGER__",
        render_serve_source_manager(
            serve_source_payload,
            messages,
            normalized_locale,
            adapter_defaults or {},
        )
        if normalized_mode == "serve"
        else "",
    )
    payload = payload.replace("__ECHARTS_SCRIPT__", render_echarts_script(normalized_mode))
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
            json.dumps(render_options, ensure_ascii=False)
        ),
    )
    return payload


def render_serve_html(
    report: dict[str, Any],
    locale: str = "en",
    sources: list[dict[str, Any]] | None = None,
    adapter_defaults: dict[str, str] | None = None,
) -> str:
    return render_html(
        report,
        locale=locale,
        mode="serve",
        sources=sources,
        adapter_defaults=adapter_defaults,
    )


def normalize_render_mode(mode: object) -> str:
    text = str(mode or "report").strip().lower()
    if text in {"report", "serve"}:
        return text
    raise ValueError(f"unsupported HTML render mode: {mode}; supported modes: report, serve")


def render_serve_source_manager(
    sources: list[dict[str, Any]],
    messages: dict[str, str],
    locale: str,
    adapter_defaults: dict[str, str],
) -> str:
    count = len(sources)
    source_word = messages["serve_source_count"]
    if count != 1:
        source_word = messages["serve_sources_count"]
    return replace_template_tokens(
        load_asset_text("serve_source_manager.html"),
        {
            "SOURCE_COUNT": str(count),
            "SOURCE_WORD": escape(source_word),
            "LATEST_SNAPSHOTS": escape(messages["serve_latest_snapshots"]),
            "REFRESH": escape(messages["serve_refresh"]),
            "SOURCE_MANAGER": escape(messages["serve_source_manager"]),
            "LANGUAGE_CONTROL": render_language_control(messages, locale),
            "DROP_COPY": escape(messages["serve_drop_copy"]),
            "CLOSE": escape(messages["close"]),
            "ADD_SOURCE": escape(messages["serve_add_source"]),
            "ADAPTER_DEFAULT_DB_PANEL": render_adapter_default_db_panel(
                messages,
                adapter_defaults,
            ),
            "SOURCE_FORMS": "".join(
                [
                    render_source_add_form("path", messages, adapter_defaults),
                    render_source_add_form("db", messages, adapter_defaults),
                    render_source_add_form("input_table", messages, adapter_defaults),
                    render_upload_form(messages, adapter_defaults),
                ]
            ),
            "SOURCES": escape(messages["serve_sources"]),
            "RELOAD": escape(messages["serve_reload"]),
            "SOURCE_LIST_ITEMS": render_source_list_items(sources, messages),
        },
    )


def render_adapter_default_db_panel(
    messages: dict[str, str],
    adapter_defaults: dict[str, str],
) -> str:
    adapter_ids = available_adapter_ids()
    if not adapter_ids:
        return ""
    selected = next(
        (adapter_id for adapter_id in adapter_ids if adapter_id in adapter_defaults),
        adapter_ids[0],
    )
    options = "".join(
        render_adapter_default_db_option(adapter_id, selected, adapter_defaults)
        for adapter_id in adapter_ids
    )
    default_db = adapter_defaults.get(selected, "")
    return f"""
      <section class="adapter-default-db-panel" aria-label="{escape(messages["serve_adapter_default_db"])}">
        <div>
          <strong>{escape(messages["serve_adapter_default_db"])}</strong>
          <p class="copy">{escape(messages["serve_adapter_default_db_copy"])}</p>
        </div>
        <form class="adapter-default-db-form" data-adapter-default-db-form>
          <label>{escape(messages["serve_adapter"])}
            <select name="adapter" data-adapter-default-db-select>
              {options}
            </select>
          </label>
          <label>{escape(messages["serve_default_db"])}
            <input name="default_db_path" value="{escape(default_db)}" autocomplete="off" data-adapter-default-db-input>
          </label>
          <div class="adapter-default-db-actions">
            <button class="step-toggle-button" type="button" data-adapter-default-db-clear>{escape(messages["serve_clear_adapter_default_db"])}</button>
            <button class="step-toggle-button primary" type="submit">{escape(messages["serve_save_adapter_default_db"])}</button>
          </div>
        </form>
      </section>"""


def render_adapter_default_db_option(
    adapter_id: str,
    selected_adapter: str,
    adapter_defaults: dict[str, str],
) -> str:
    default_db = adapter_defaults.get(adapter_id)
    default_attr = f' data-default-db="{escape(default_db)}"' if default_db else ""
    selected = " selected" if adapter_id == selected_adapter else ""
    return f'<option value="{escape(adapter_id)}"{selected}{default_attr}>{escape(adapter_id)}</option>'


def render_echarts_script(mode: str) -> str:
    cdn = escape(ECHARTS_CDN_SRC)
    if mode == "serve":
        local = escape(ECHARTS_LOCAL_SRC)
        return (
            f'<script src="{local}" '
            f'onerror="this.onerror=null;this.src=\'{cdn}\'"></script>'
        )
    return f'<script src="{cdn}"></script>'


def render_language_control(messages: dict[str, str], locale: str) -> str:
    options = [
        ("en", messages["language_en"]),
        ("zh-CN", messages["language_zh_cn"]),
    ]
    option_html = "".join(
        f'<option value="{escape(value)}" {"selected" if value == locale else ""}>{escape(label)}</option>'
        for value, label in options
    )
    return f"""
      <label class="serve-language-select">
        <span>{escape(messages["language"])}</span>
        <select data-locale-select aria-label="{escape(messages["language"])}">
          {option_html}
        </select>
      </label>"""


def render_source_add_form(
    kind: str,
    messages: dict[str, str],
    adapter_defaults: dict[str, str],
) -> str:
    label_key = {
        "path": "serve_path_source",
        "db": "serve_db_source",
        "input_table": "serve_input_table_source",
    }[kind]
    name = "input_table" if kind == "input_table" else kind
    field_tag = (
        f'<textarea name="{escape(name)}" autocomplete="off" required rows="2"></textarea>'
        if kind in {"path", "db"}
        else f'<input name="{escape(name)}" autocomplete="off" required>'
    )
    session_field = ""
    if kind == "db":
        session_field = f"""
            <label>{escape(messages["serve_session_id"])}
              <input name="session_id" autocomplete="off">
            </label>"""
    inspect_button = ""
    picker = ""
    if kind == "db":
        inspect_button = f"""
              <button class="step-toggle-button" type="button" data-db-inspect>{escape(messages["serve_inspect_db"])}</button>"""
        picker = f"""
            <div class="db-session-picker" data-db-session-picker hidden></div>"""
    return f"""
          <form class="source-form" data-source-add-form data-source-kind="{escape(kind)}">
            <label>{escape(messages[label_key])}
              {field_tag}
            </label>
            <label>{escape(messages["serve_source_alias"])}
              <input name="alias" autocomplete="off">
            </label>
            {session_field}
            <div class="source-form-actions">
              {inspect_button}
              <span class="source-add-actions">
                {render_adapter_select(messages, adapter_defaults)}
                <button class="step-toggle-button" type="submit">{escape(messages["serve_add_source"])}</button>
              </span>
            </div>
            {picker}
          </form>"""


def render_upload_form(messages: dict[str, str], adapter_defaults: dict[str, str]) -> str:
    return f"""
          <form class="source-form upload-form" data-source-upload-form>
            <strong>{escape(messages["serve_upload_snapshot"])}</strong>
            <label>{escape(messages["serve_upload_file"])}
              <input name="file" type="file" accept=".json,.jsonl,application/json,application/x-ndjson" required>
            </label>
            <label>{escape(messages["serve_source_alias"])}
              <input name="alias" autocomplete="off">
            </label>
            <div class="source-form-actions">
              <span class="source-add-actions">
                {render_adapter_select(messages, adapter_defaults)}
                <button class="step-toggle-button" type="submit">{escape(messages["serve_upload"])}</button>
              </span>
            </div>
          </form>"""


def render_adapter_select(messages: dict[str, str], adapter_defaults: dict[str, str]) -> str:
    options = [
        ("auto", messages["serve_adapter_auto"]),
        *[(adapter_id, adapter_id) for adapter_id in available_adapter_ids()],
    ]
    option_html = "".join(
        render_adapter_option(value, label, adapter_defaults)
        for value, label in options
    )
    return f"""
              <label class="source-adapter-select">
                <span>{escape(messages["serve_adapter"])}</span>
                <select name="adapter" aria-label="{escape(messages["serve_adapter"])}">
                  {option_html}
                </select>
              </label>"""


def render_adapter_option(
    value: str,
    label: str,
    adapter_defaults: dict[str, str],
) -> str:
    default_db = adapter_defaults.get(value)
    default_attr = f' data-default-db="{escape(default_db)}"' if default_db else ""
    selected = "selected" if value == "auto" else ""
    return f'<option value="{escape(value)}" {selected}{default_attr}>{escape(label)}</option>'


def render_source_list_items(
    sources: list[dict[str, Any]],
    messages: dict[str, str],
) -> str:
    if not sources:
        return f'<li class="source-row empty">{escape(messages["serve_no_sources"])}</li>'
    return "".join(render_source_list_item(source, messages) for source in sources)


def render_source_list_item(
    source: dict[str, Any],
    messages: dict[str, str],
) -> str:
    label = str(source.get("label") or source.get("source_key") or "source")
    alias = str(source.get("source_alias") or "")
    display_label = alias or label
    kind = str(source.get("kind") or "source")
    adapter = str(source.get("adapter") or "-")
    status = str(source.get("last_status") or "-")
    active = bool(source.get("active", True))
    snapshot = bool(source.get("snapshot", False))
    refreshable = bool(source.get("refreshable", not snapshot))
    source_key = str(source.get("source_key") or "")
    archive_label = messages["serve_archive"] if active else messages["serve_activate"]
    archive_action = "archive" if active else "activate"
    refresh_button = (
        f'<button type="button" data-source-action="refresh" data-source-key="{escape(source_key)}">{escape(messages["serve_refresh"])}</button>'
        if refreshable and source_key
        else f'<span>{escape(messages["serve_snapshot"])}</span>'
    )
    archive_button = (
        f'<button type="button" data-source-action="{escape(archive_action)}" data-source-key="{escape(source_key)}">{escape(archive_label)}</button>'
        if source_key
        else ""
    )
    delete_button = (
        f'<button type="button" data-source-action="delete" data-source-key="{escape(source_key)}">{escape(messages["serve_delete"])}</button>'
        if source_key
        else ""
    )
    state_label = messages["serve_active"] if active else messages["serve_archived"]
    return f"""
            <li class="source-row {'archived' if not active else ''}">
              <div class="source-row-main">
                <strong>{escape(display_label)}</strong>
                {render_source_origin(label, alias)}
                <span>{escape(kind)} / {escape(adapter)} / {escape(status)} / {escape(state_label)}</span>
                <label class="source-alias-edit">
                  <span>{escape(messages["serve_source_alias"])}</span>
                  <input data-source-alias-input data-source-key="{escape(source_key)}" value="{escape(alias)}" autocomplete="off">
                  <button type="button" data-source-alias-save data-source-key="{escape(source_key)}">{escape(messages["serve_save_alias"])}</button>
                </label>
              </div>
              <div class="source-row-actions">{refresh_button}{archive_button}{delete_button}</div>
            </li>"""


def render_source_origin(label: str, alias: str) -> str:
    if not alias:
        return ""
    return f'<span class="source-origin">{escape(label)}</span>'


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
    if name in ASSET_BUNDLES:
        return "\n".join(load_asset_text(part) for part in ASSET_BUNDLES[name])
    return files(ASSET_PACKAGE).joinpath(name).read_text(encoding="utf-8")


def replace_template_tokens(template: str, values: dict[str, str]) -> str:
    rendered = template
    for key, value in values.items():
        rendered = rendered.replace(f"__{key}__", value)
    return rendered


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


def safe_json_for_script(value: str) -> str:
    return value.replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e")
