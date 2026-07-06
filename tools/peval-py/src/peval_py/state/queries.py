from __future__ import annotations

from typing import Any

from peval_py._state.annotations import (
    meta_with_source_metadata,
    optional_int,
    optional_str,
    source_report_with_current_annotations,
    uniquify_trial_keys,
)
from peval_py._state.artifacts import workspace_analysis_eval_slug
from peval_py.config import ToolConfig
from peval_py.report import build_report_from_snapshots, empty_report
from peval_py.state.constants import SOURCE_STATUS_MISSING


class StateQueryMixin:
    def source_by_key(self, source_key: str) -> dict[str, Any]:
        for row in self.source_rows(active_only=False):
            if row.get("source_key") == source_key:
                return row
        raise ValueError(f"unknown source: {source_key}")

    def active_report(
        self,
        config: ToolConfig | None = None,
        *,
        source_keys: list[str] | None = None,
        source_state: str = "active",
    ) -> dict[str, Any]:
        annotation_config = self.annotation_config(config)
        active_filter: bool | None = None
        if source_keys is None:
            if source_state == "active":
                active_filter = True
            elif source_state == "archived":
                active_filter = False
            elif source_state == "all":
                active_filter = None
            else:
                raise ValueError("source_state must be active, archived, or all")
        rows = self.source_rows(
            source_keys=source_keys,
            active=active_filter,
        )
        rows = [row for row in rows if row.get("artifact_dir")]
        if source_keys:
            found = {str(row.get("source_key")) for row in rows}
            missing = [key for key in source_keys if key not in found]
            if missing:
                raise ValueError(f"unknown source: {missing[0]}")
        if not rows:
            return empty_report("serve")
        readable_rows: list[dict[str, Any]] = []
        stored: list[dict[str, dict[str, Any]]] = []
        errors: list[str] = []
        for row in rows:
            try:
                stored.append(self.read_trial_artifacts(row))
                readable_rows.append(row)
            except Exception as exc:  # noqa: BLE001 - tolerate missing artifacts in full serve reports.
                errors.append(f"{row.get('source_key')}: {exc}")
        if errors and source_keys:
            raise ValueError(errors[0])
        if not readable_rows:
            return empty_report("serve")
        trajectories = [item["trajectory"] for item in stored]
        metas = uniquify_trial_keys(
            [
                meta_with_source_metadata(
                    item["meta"],
                    row.get("source_alias"),
                    row.get("source_tags"),
                )
                for row, item in zip(readable_rows, stored, strict=True)
            ]
        )
        reports = [
            source_report_with_current_annotations(
                dict(row),
                trajectory,
                meta,
                annotation_config,
            )
            for row, trajectory, meta in zip(
                readable_rows,
                trajectories,
                metas,
                strict=True,
            )
        ]
        return build_report_from_snapshots(
            trajectories,
            metas,
            input_label="serve",
            source_reports=reports,
        )

    def annotation_config(self, config: ToolConfig | None) -> ToolConfig:
        if config is not None and config.workspace_root:
            return config
        eval_slug = (
            config.analysis_eval_slug
            if config is not None
            else workspace_analysis_eval_slug(self.paths)
        )
        if config is None:
            return ToolConfig(
                workspace_root=str(self.paths.root),
                analysis_eval_slug=eval_slug,
            )
        return ToolConfig(
            adapter=config.adapter,
            locale=config.locale,
            workspace_root=str(self.paths.root),
            analysis_eval_slug=eval_slug,
            agent_name=config.agent_name,
            agent_version=config.agent_version,
            model=config.model,
            max_content_chars=config.max_content_chars,
            max_content_chars_explicit=config.max_content_chars_explicit,
            redact=config.redact,
            db=config.db,
            adapter_options=config.adapter_options,
            adapter_options_by_id=config.adapter_options_by_id,
            adapter_default_db_paths=config.adapter_default_db_paths,
        )

    def source_rows(
        self,
        *,
        source_keys: list[str] | None = None,
        active_only: bool = False,
        active: bool | None = None,
    ) -> list[dict[str, Any]]:
        wanted = set(source_keys or [])
        rows = [
            self.source_row_from_cell_dir(cell_dir)
            for cell_dir in self.discover_source_cell_dirs()
        ]
        rows = [row for row in rows if row.get("source_key")]
        if wanted:
            rows = [row for row in rows if row.get("source_key") in wanted]
        if active is not None:
            rows = [row for row in rows if bool(row.get("active")) is active]
        elif active_only:
            rows = [row for row in rows if bool(row.get("active"))]
        rows.sort(key=lambda row: (int(row.get("created_at_ms") or 0), str(row.get("source_key") or "")))
        return rows

    def source_payload(self) -> list[dict[str, Any]]:
        payload: list[dict[str, Any]] = []
        for row in self.source_rows(active_only=False):
            item = dict(row)
            item["refreshable"] = bool(item["refreshable"])
            item["active"] = bool(item["active"])
            item["snapshot"] = bool(item["snapshot"])
            item["trial_key"] = optional_str(item.get("trial_key"))
            item["trial_session_id"] = optional_str(item.get("trial_session_id"))
            item["last_turn_finished_at_ms"] = optional_int(
                item.get("last_turn_finished_at_ms")
            )
            if item.get("artifact_dir") and self.artifact_missing(item):
                item["last_status"] = SOURCE_STATUS_MISSING
                item["last_error"] = self.missing_artifact_message(item)
            payload.append(item)
        return payload
