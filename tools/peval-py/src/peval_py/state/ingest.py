from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path
from typing import Any

from peval_py._state.annotations import (
    is_report_json,
    matching_annotation_items,
    merged_analysis_json,
    merged_analysis_markdown,
    merged_note_markdown,
    optional_str,
    parsed_object,
)
from peval_py._state.artifacts import (
    read_json_object,
    relative_to_root,
    source_key_for_trial,
    trial_artifacts,
    workspace_analysis_eval_slug,
    write_json_file,
    write_text_file,
)
from peval_py._state.sources import (
    loaded_session_from_source,
    source_row_for_session,
    trial_payload_from_report,
)
from peval_py.analysis import write_note_file
from peval_py.atif import convert_atif_trajectory, is_atif_trajectory
from peval_py.config import ToolConfig, config_for_adapter
from peval_py.models import LoadedInputs, LoadedSession
from peval_py.pipeline import report_session_for_loaded
from peval_py.report import ReportSession, build_multi_report
from peval_py.sources import read_jsonl_text
from peval_py.state.constants import (
    SOURCE_STATUS_MISSING,
    SOURCE_STATUS_OK,
    UPLOAD_LIMIT_BYTES,
)
from peval_py.state.summaries import now_ms, trial_summary


class StateIngestMixin:
    def upsert_loaded_sources(
        self,
        loaded_inputs: LoadedInputs,
        config: ToolConfig,
    ) -> list[str]:
        return self.import_loaded_sources(loaded_inputs, config)

    def upsert_loaded_source(
        self,
        session: LoadedSession,
        config: ToolConfig,
        *,
        commit: bool = True,
        timestamp: int | None = None,
    ) -> str:
        del commit, timestamp
        return self.import_loaded_sources(
            LoadedInputs(sessions=[session], notes=[]),
            config,
        )[0]

    def import_loaded_sources(
        self,
        loaded_inputs: LoadedInputs,
        config: ToolConfig,
    ) -> list[str]:
        prepared: dict[
            str,
            tuple[
                dict[str, Any],
                dict[str, Any],
                dict[str, Any],
                int,
                str,
                bool,
                bool,
                Path | None,
            ],
        ] = {}
        ordered_keys: list[str] = []
        for session in loaded_inputs.sessions:
            source = source_row_for_session(session)
            eval_slug = session.artifact_eval_slug or config.analysis_eval_slug
            refreshable = session.snapshot_trajectory is None
            snapshot = not refreshable
            sidecar_source = None
            if session.snapshot_trajectory is not None:
                if session.snapshot_meta is None:
                    raise ValueError("snapshot source is missing Trial metadata")
                trajectory = deepcopy(session.snapshot_trajectory)
                meta = deepcopy(session.snapshot_meta)
                if session.source_kind == "trial-artifact" and session.input_path:
                    sidecar_source = Path(session.input_path)
            else:
                report_session = report_session_for_loaded(session, config)
                report = build_multi_report([report_session], config, [])
                trajectory, meta = trial_payload_from_report(report)
            source_key = source_key_for_trial(
                eval_slug,
                source,
                trajectory,
                meta,
            )
            warnings = meta.get("warnings") or []
            if source_key not in prepared:
                ordered_keys.append(source_key)
            prepared[source_key] = (
                source,
                trajectory,
                meta,
                len(warnings),
                eval_slug,
                refreshable,
                snapshot,
                sidecar_source,
            )

        timestamp = now_ms()
        try:
            for source_key in ordered_keys:
                (
                    source,
                    trajectory,
                    meta,
                    warning_count,
                    eval_slug,
                    refreshable,
                    snapshot,
                    sidecar_source,
                ) = prepared[source_key]
                artifact_dir = self.store_trial(
                    trajectory,
                    meta,
                    eval_slug,
                    source=source,
                )
                if sidecar_source is not None:
                    self.copy_trial_sidecars(sidecar_source, artifact_dir)
                self.upsert_source_row(
                    source_key,
                    source,
                    artifact_dir,
                    timestamp,
                    trajectory=trajectory,
                    meta=meta,
                    refreshable=refreshable,
                    snapshot=snapshot,
                    status=SOURCE_STATUS_OK,
                )
                self.log_refresh(source_key, SOURCE_STATUS_OK, warning_count, None, timestamp)
            self.conn.commit()
        except Exception:
            self.conn.rollback()
            raise
        return ordered_keys

    def refresh_sources(self, source_keys: list[str] | None, config: ToolConfig) -> None:
        rows = self.source_rows(source_keys=source_keys, active_only=False)
        for row in rows:
            if not row["refreshable"]:
                continue
            self.refresh_source(row, config)

    def refresh_source(self, source: dict[str, Any], config: ToolConfig) -> None:
        source_key = source["source_key"]
        timestamp = now_ms()
        try:
            session = loaded_session_from_source(source)
            report_session = report_session_for_loaded(session, config)
            report = build_multi_report([report_session], config, [])
            artifact_dir, warning_count = self.store_report_for_source(
                source_key,
                report,
                config,
                source=source,
            )
            self.conn.execute(
                """
                UPDATE peval_py_sources
                SET artifact_dir = ?, artifact_updated_at_ms = ?,
                    last_status = ?, last_error = NULL, last_refreshed_at_ms = ?,
                    updated_at_ms = ?
                WHERE source_key = ?
                """,
                (artifact_dir, timestamp, SOURCE_STATUS_OK, timestamp, timestamp, source_key),
            )
            self.update_source_summary(source_key, report["trajectory"][0], report["trajectory_meta"][0])
            self.log_refresh(source_key, SOURCE_STATUS_OK, warning_count, None, timestamp)
        except Exception as exc:  # noqa: BLE001 - state boundary.
            self.conn.execute(
                """
                UPDATE peval_py_sources
                SET last_status = ?, last_error = ?, last_refreshed_at_ms = ?,
                    updated_at_ms = ?
                WHERE source_key = ?
                """,
                ("error", str(exc), timestamp, timestamp, source_key),
            )
            self.log_refresh(source_key, "error", 0, str(exc), timestamp)
        self.conn.commit()

    def store_report_for_source(
        self,
        source_key: str,
        report: dict[str, Any],
        config: ToolConfig,
        *,
        source: dict[str, Any] | None = None,
    ) -> tuple[str, int]:
        source = source or self.source_by_key(source_key)
        trajectory, meta = trial_payload_from_report(report)
        refreshed_source_key = source_key_for_trial(
            config.analysis_eval_slug,
            source,
            trajectory,
            meta,
        )
        if refreshed_source_key != source_key:
            raise ValueError(
                "refreshed source resolved to a different Trial cell; "
                f"expected {source_key}, got {refreshed_source_key}"
            )
        artifact_dir = self.store_trial(
            trajectory,
            meta,
            config.analysis_eval_slug,
            source=source,
        )
        return artifact_dir, len(meta.get("warnings") or [])

    def ingest_upload(
        self,
        filename: str,
        content: str,
        config: ToolConfig,
        adapter: str | None = None,
    ) -> list[str]:
        if len(content.encode("utf-8")) > UPLOAD_LIMIT_BYTES:
            raise ValueError("uploaded source exceeds 20 MiB limit")
        label = Path(filename or "upload").name
        parsed_json: Any = None
        if label.endswith(".json"):
            try:
                parsed_json = json.loads(content)
            except json.JSONDecodeError:
                parsed_json = None
        if isinstance(parsed_json, dict) and is_report_json(parsed_json):
            return self.ingest_report_snapshot(
                parsed_json,
                label,
                config,
                materialize_annotations=True,
            )
        if isinstance(parsed_json, dict) and is_atif_trajectory(parsed_json):
            conversion = convert_atif_trajectory(parsed_json)
            report = build_multi_report(
                [
                    ReportSession(
                        conversion=conversion,
                        input_label=label,
                        adapter_id="atif",
                    )
                ],
                config_for_adapter(config, "atif"),
                [],
            )
            return self.ingest_report_snapshot(report, label, config, adapter="atif")
        if not label.endswith(".jsonl"):
            raise ValueError("uploaded source must be JSONL, ATIF JSON, or report JSON")
        source_config = config_for_adapter(config, adapter or config.adapter)
        records = read_jsonl_text(content)
        session = LoadedSession(
            records=records,
            input_label=label,
            adapter_id=source_config.adapter,
            session_hint=Path(label).stem or "session",
            source_kind="upload",
        )
        report = build_multi_report(
            [report_session_for_loaded(session, source_config)],
            source_config,
            [],
        )
        return self.ingest_report_snapshot(
            report,
            label,
            source_config,
            adapter=source_config.adapter,
        )

    def ingest_report_snapshot(
        self,
        report: dict[str, Any],
        label: str,
        config: ToolConfig | None = None,
        *,
        adapter: str | None = None,
        materialize_annotations: bool = False,
    ) -> list[str]:
        trajectories = report.get("trajectory")
        metas = report.get("trajectory_meta")
        if not isinstance(trajectories, list) or not isinstance(metas, list):
            raise ValueError(
                "report JSON snapshot must contain trajectory and trajectory_meta arrays"
            )
        if len(trajectories) != len(metas):
            raise ValueError("report JSON snapshot trajectory/meta counts differ")
        eval_slug = (
            config.analysis_eval_slug
            if config is not None
            else workspace_analysis_eval_slug(self.paths)
        )
        prepared: dict[str, tuple[dict[str, Any], dict[str, Any], dict[str, Any], int]] = {}
        ordered_keys: list[str] = []
        for index, (trajectory, meta) in enumerate(
            zip(trajectories, metas, strict=True),
            start=1,
        ):
            if not isinstance(trajectory, dict) or not isinstance(meta, dict):
                raise ValueError("report JSON snapshot contains non-object Trial data")
            source_label = (
                f"{label}:{trajectory.get('session_id') or meta.get('trial_key') or index}"
            )
            source = {
                "kind": "snapshot",
                "adapter": adapter or optional_str(meta.get("adapter")) or "snapshot",
                "label": source_label,
                "input_path": None,
                "db_path": None,
                "session_id": optional_str(
                    trajectory.get("session_id") or meta.get("trial_key")
                ),
                "source_alias": None,
                "agent_name": None,
                "agent_version": None,
                "model": None,
            }
            source_key = source_key_for_trial(
                eval_slug,
                source,
                trajectory,
                meta,
            )
            if source_key not in prepared:
                ordered_keys.append(source_key)
            prepared[source_key] = (
                source,
                trajectory,
                meta,
                len(meta.get("warnings") or []),
            )

        timestamp = now_ms()
        try:
            for source_key in ordered_keys:
                source, trajectory, meta, warning_count = prepared[source_key]
                artifact_dir = self.store_trial(
                    trajectory,
                    meta,
                    eval_slug,
                    source=source,
                )
                if materialize_annotations:
                    self.materialize_snapshot_annotations(report, meta, artifact_dir)
                self.upsert_source_row(
                    source_key,
                    source,
                    artifact_dir,
                    timestamp,
                    trajectory=trajectory,
                    meta=meta,
                    refreshable=False,
                    snapshot=True,
                    status=SOURCE_STATUS_OK,
                )
                self.log_refresh(
                    source_key,
                    SOURCE_STATUS_OK,
                    warning_count,
                    None,
                    timestamp,
                )
            self.conn.commit()
        except Exception:
            self.conn.rollback()
            raise
        return ordered_keys

    def materialize_snapshot_annotations(
        self,
        report: dict[str, Any],
        meta: dict[str, Any],
        artifact_dir: str,
    ) -> None:
        annotations = parsed_object(report.get("annotations"))
        trial_key = str(meta.get("trial_key") or "")
        cell_dir = self.resolve_artifact_dir(artifact_dir)

        notes = matching_annotation_items(annotations, "notes", trial_key)
        note_markdown = merged_note_markdown(notes)
        if note_markdown:
            write_note_file(cell_dir / "notes.md", self.paths.root, note_markdown)

        analyses = matching_annotation_items(annotations, "analysis", trial_key)
        analysis_payload = merged_analysis_json(analyses)
        if analysis_payload is not None:
            write_json_file(cell_dir / "analysis.json", analysis_payload)
        analysis_markdown = merged_analysis_markdown(analyses)
        if analysis_markdown:
            write_text_file(cell_dir / "analysis.md", analysis_markdown)

    def sync_artifact_sources(self, config: ToolConfig | None = None) -> list[str]:
        eval_slug = (
            config.analysis_eval_slug
            if config is not None
            else workspace_analysis_eval_slug(self.paths)
        )
        timestamp = now_ms()
        seen_keys: list[str] = []
        try:
            for cell_dir in self.discover_trial_cell_dirs(eval_slug):
                artifacts = trial_artifacts(cell_dir)
                try:
                    trajectory = read_json_object(artifacts.trajectory_path)
                    meta = read_json_object(artifacts.meta_path)
                except Exception:
                    continue
                source = self.source_row_for_artifact_cell(cell_dir, trajectory, meta)
                source_key = source_key_for_trial(eval_slug, source, trajectory, meta)
                artifact_dir = relative_to_root(self.paths.root, cell_dir)
                if self.source_exists(source_key):
                    self.update_existing_artifact_source(
                        source_key,
                        artifact_dir,
                        timestamp,
                        trajectory,
                        meta,
                    )
                else:
                    self.upsert_source_row(
                        source_key,
                        source,
                        artifact_dir,
                        timestamp,
                        trajectory=trajectory,
                        meta=meta,
                        refreshable=False,
                        snapshot=True,
                        status=SOURCE_STATUS_OK,
                    )
                seen_keys.append(source_key)
            self.mark_missing_artifact_sources(timestamp)
            self.conn.commit()
        except Exception:
            self.conn.rollback()
            raise
        return seen_keys

    def source_exists(self, source_key: str) -> bool:
        row = self.conn.execute(
            "SELECT 1 FROM peval_py_sources WHERE source_key = ?",
            (source_key,),
        ).fetchone()
        return row is not None

    def update_existing_artifact_source(
        self,
        source_key: str,
        artifact_dir: str,
        timestamp: int,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
    ) -> None:
        summary = trial_summary(trajectory, meta)
        self.conn.execute(
            """
            UPDATE peval_py_sources
            SET artifact_dir = ?,
                artifact_updated_at_ms = ?,
                trial_key = ?,
                trial_session_id = ?,
                last_turn_finished_at_ms = ?,
                last_status = CASE
                    WHEN last_status = ? THEN ?
                    ELSE last_status
                END,
                last_error = CASE
                    WHEN last_status = ? THEN NULL
                    ELSE last_error
                END,
                updated_at_ms = ?
            WHERE source_key = ?
            """,
            (
                artifact_dir,
                timestamp,
                summary["trial_key"],
                summary["trial_session_id"],
                summary["last_turn_finished_at_ms"],
                SOURCE_STATUS_MISSING,
                SOURCE_STATUS_OK,
                SOURCE_STATUS_MISSING,
                timestamp,
                source_key,
            ),
        )

    def mark_missing_artifact_sources(self, timestamp: int) -> None:
        for row in self.source_rows(active_only=False):
            if not row.get("artifact_dir") or not self.artifact_missing(row):
                continue
            self.conn.execute(
                """
                UPDATE peval_py_sources
                SET last_status = ?, last_error = ?, updated_at_ms = ?
                WHERE source_key = ?
                """,
                (
                    SOURCE_STATUS_MISSING,
                    self.missing_artifact_message(row),
                    timestamp,
                    row["source_key"],
                ),
            )
