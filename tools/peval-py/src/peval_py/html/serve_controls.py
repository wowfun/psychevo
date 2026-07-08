from __future__ import annotations

from html import escape
from typing import Any

from peval_py.adapters import available_adapter_ids
from peval_py.html.assets import load_asset_text, replace_template_tokens

def render_serve_source_manager(
    sources: list[dict[str, Any]],
    messages: dict[str, str],
    locale: str,
    adapter_defaults: dict[str, str],
    *,
    loading: bool = False,
) -> str:
    count = len(sources)
    source_word = messages["serve_source_count"]
    if count != 1:
        source_word = messages["serve_sources_count"]
    source_summary = (
        messages["serve_loading_sources"]
        if loading
        else f"{count} {source_word}"
    )
    source_status = (
        messages["serve_scanning_runs"]
        if loading
        else messages["serve_latest_snapshots"]
    )
    return replace_template_tokens(
        load_asset_text("serve_source_manager.html"),
        {
            "SOURCE_SUMMARY": escape(source_summary),
            "SOURCE_STATUS": escape(source_status),
            "SOURCE_STATUS_CLASS": "loading" if loading else "",
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
            "SOURCE_LIST_ITEMS": render_source_list_items(sources, messages, loading=loading),
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
    if kind == "path":
        field_tag = f'<textarea name="{escape(name)}" autocomplete="off" required rows="4" data-path-picker-target></textarea>'
    elif kind == "db":
        field_tag = f'<textarea name="{escape(name)}" autocomplete="off" required rows="2"></textarea>'
    else:
        field_tag = f'<input name="{escape(name)}" autocomplete="off" required>'
    path_picker = ""
    if kind == "path":
        path_picker = f"""
            <div class="source-picker-actions">
              <button class="step-toggle-button" type="button" data-path-picker>{escape(messages["serve_choose_path_files"])}</button>
            </div>"""
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
            {path_picker}
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
    *,
    loading: bool = False,
) -> str:
    if loading:
        return f'<li class="source-row empty loading">{escape(messages["serve_scanning_runs"])}</li>'
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
