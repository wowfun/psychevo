from __future__ import annotations

import hashlib
import json
import math
import os
import shutil
import sqlite3
import time
import tomllib
from copy import deepcopy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.analysis import (
    MERGEABLE_ANALYSIS_LIST_FIELDS,
    cached_analysis_report,
    cached_note_report,
    write_note_file,
)
from peval_py.atif import convert_atif_trajectory, is_atif_trajectory
from peval_py.config import ToolConfig, config_for_adapter, default_workspace_config_text
from peval_py.inputs import AdapterAssignments, LoadedInputs, LoadedSession, load_inputs
from peval_py.pipeline import report_session_for_loaded
from peval_py.report import (
    ReportSession,
    build_multi_report,
    build_report_from_snapshots,
    empty_report,
)
from peval_py.sources import read_jsonl_text

PEVAL_PY_CONFIG = "peval-py.toml"
PEVAL_ROOT_ENV = "PEVAL_ROOT"
STATE_SCHEMA_VERSION = 3
DEFAULT_ANALYSIS_EVAL_SLUG = "default"
AGENT_DIR = "agent"
TRAJECTORY_FILENAME = "trajectory.json"
TRAJECTORY_META_FILENAME = "trajectory_meta.json"
UPLOAD_LIMIT_BYTES = 20 * 1024 * 1024
REFRESH_LOG_LIMIT = 200


@dataclass(frozen=True)
class WorkspacePaths:
    root: Path
    config_path: Path
    state_db_path: Path


@dataclass(frozen=True)
class TrialArtifacts:
    trajectory_path: Path
    meta_path: Path


def now_ms() -> int:
    return int(time.time() * 1000)


def resolve_workspace_root(explicit_root: str | None = None) -> Path:
    if explicit_root:
        return Path(explicit_root).expanduser().resolve()
    env_root = os.environ.get(PEVAL_ROOT_ENV)
    if env_root:
        return Path(env_root).expanduser().resolve()
    discovered = discover_workspace_root(Path.cwd())
    if discovered is not None:
        return discovered
    raise ValueError(
        "peval-py workspace is not initialized; run `peval-py init`, "
        f"pass --root/-r, or set {PEVAL_ROOT_ENV}"
    )


def discover_workspace_root(start: Path) -> Path | None:
    current = start.resolve()
    while True:
        candidate = current / PEVAL_PY_CONFIG
        if candidate.is_file():
            return ensure_workspace_root(current)
        if current.parent == current:
            return None
        current = current.parent


def ensure_workspace_root(root: Path) -> Path:
    resolved = root.resolve()
    config_path = resolved / PEVAL_PY_CONFIG
    if not config_path.is_file():
        raise ValueError(
            f"{resolved} is not an initialized peval-py workspace; "
            f"run `peval-py init -r {resolved}`"
        )
    try:
        tomllib.loads(config_path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"failed to parse {config_path}: {exc}") from exc
    return resolved


def workspace_paths(root: Path) -> WorkspacePaths:
    root = root.expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    config_path = root / PEVAL_PY_CONFIG
    state_db_path = root / "state.db"
    if config_path.is_file():
        try:
            data = tomllib.loads(config_path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as exc:
            raise ValueError(f"failed to parse {config_path}: {exc}") from exc
        raw_state_db = data.get("state_db", "state.db")
        state_db_path = Path(str(raw_state_db)).expanduser()
        if not state_db_path.is_absolute():
            state_db_path = root / state_db_path
    else:
        config_path.write_text(default_workspace_config_text(), encoding="utf-8")
    return WorkspacePaths(root=root, config_path=config_path, state_db_path=state_db_path)


class ServeStateStore:
    def __init__(
        self,
        paths: WorkspacePaths,
        *,
        initialize: bool = True,
        readonly: bool = False,
    ) -> None:
        self.paths = paths
        if readonly:
            uri = self.paths.state_db_path.resolve().as_uri() + "?mode=ro"
            self.conn = sqlite3.connect(uri, uri=True, check_same_thread=False)
        else:
            self.paths.state_db_path.parent.mkdir(parents=True, exist_ok=True)
            self.conn = sqlite3.connect(self.paths.state_db_path, check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        if initialize:
            self.initialize_schema()

    def close(self) -> None:
        self.conn.close()

    def initialize_schema(self) -> None:
        self.conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS peval_py_sources (
                source_key TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                adapter TEXT NOT NULL,
                label TEXT NOT NULL,
                input_path TEXT,
                db_path TEXT,
                session_id TEXT,
                source_alias TEXT,
                agent_name TEXT,
                agent_version TEXT,
                model TEXT,
                artifact_dir TEXT,
                artifact_updated_at_ms INTEGER,
                refreshable INTEGER NOT NULL,
                active INTEGER NOT NULL,
                snapshot INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_status TEXT,
                last_error TEXT,
                last_refreshed_at_ms INTEGER
            );
            CREATE TABLE IF NOT EXISTS peval_py_refresh_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_key TEXT,
                status TEXT NOT NULL,
                warning_count INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                refreshed_at_ms INTEGER NOT NULL
            );
            """
        )
        self.conn.commit()

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
        prepared: dict[str, tuple[dict[str, Any], dict[str, Any], dict[str, Any], int]] = {}
        ordered_keys: list[str] = []
        for session in loaded_inputs.sessions:
            source = source_row_for_session(session)
            report_session = report_session_for_loaded(session, config)
            report = build_multi_report([report_session], config, [])
            trajectory, meta = trial_payload_from_report(report)
            source_key = source_key_for_trial(
                config.analysis_eval_slug,
                source,
                trajectory,
                meta,
            )
            warnings = meta.get("warnings") or []
            if source_key not in prepared:
                ordered_keys.append(source_key)
            prepared[source_key] = (source, trajectory, meta, len(warnings))

        timestamp = now_ms()
        try:
            for source_key in ordered_keys:
                source, trajectory, meta, warning_count = prepared[source_key]
                artifact_dir = self.store_trial(
                    trajectory,
                    meta,
                    config.analysis_eval_slug,
                    source=source,
                )
                self.upsert_source_row(
                    source_key,
                    source,
                    artifact_dir,
                    timestamp,
                    refreshable=True,
                    snapshot=False,
                    status="ok",
                )
                self.log_refresh(source_key, "ok", warning_count, None, timestamp)
            self.conn.commit()
        except Exception:
            self.conn.rollback()
            raise
        return ordered_keys

    def upsert_source_row(
        self,
        source_key: str,
        source: dict[str, Any],
        artifact_dir: str,
        timestamp: int,
        *,
        refreshable: bool,
        snapshot: bool,
        status: str,
        error: str | None = None,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO peval_py_sources
            (source_key, kind, adapter, label, input_path, db_path, session_id,
             source_alias, agent_name, agent_version, model,
             artifact_dir, artifact_updated_at_ms,
             refreshable, active, snapshot, created_at_ms, updated_at_ms,
             last_status, last_error, last_refreshed_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_key) DO UPDATE SET
                kind = excluded.kind,
                adapter = excluded.adapter,
                label = excluded.label,
                input_path = excluded.input_path,
                db_path = excluded.db_path,
                session_id = excluded.session_id,
                source_alias = COALESCE(excluded.source_alias, peval_py_sources.source_alias),
                agent_name = excluded.agent_name,
                agent_version = excluded.agent_version,
                model = excluded.model,
                artifact_dir = excluded.artifact_dir,
                artifact_updated_at_ms = excluded.artifact_updated_at_ms,
                refreshable = excluded.refreshable,
                snapshot = excluded.snapshot,
                active = 1,
                updated_at_ms = excluded.updated_at_ms,
                last_status = excluded.last_status,
                last_error = excluded.last_error,
                last_refreshed_at_ms = excluded.last_refreshed_at_ms
            """,
            (
                source_key,
                source["kind"],
                source["adapter"],
                source["label"],
                source.get("input_path"),
                source.get("db_path"),
                source.get("session_id"),
                source.get("source_alias"),
                source.get("agent_name"),
                source.get("agent_version"),
                source.get("model"),
                artifact_dir,
                timestamp,
                1 if refreshable else 0,
                1 if snapshot else 0,
                timestamp,
                timestamp,
                status,
                error,
                timestamp,
            ),
        )

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
                (artifact_dir, timestamp, "ok", timestamp, timestamp, source_key),
            )
            self.log_refresh(source_key, "ok", warning_count, None, timestamp)
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

    def source_by_key(self, source_key: str) -> dict[str, Any]:
        row = self.conn.execute(
            "SELECT * FROM peval_py_sources WHERE source_key = ?",
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        return dict(row)

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

    def store_trial(
        self,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
        eval_slug: str,
        *,
        source: dict[str, Any],
    ) -> str:
        return self.write_trial_artifacts(
            source=source,
            trajectory=trajectory,
            meta=meta,
            eval_slug=eval_slug,
        )

    def write_trial_artifacts(
        self,
        *,
        source: dict[str, Any],
        trajectory: dict[str, Any],
        meta: dict[str, Any],
        eval_slug: str,
    ) -> str:
        artifact_dir = trial_cell_dir(
            self.paths.root,
            eval_slug=eval_slug,
            source=source,
            trajectory=trajectory,
            meta=meta,
        )
        artifacts = trial_artifacts(artifact_dir)
        write_json_file(artifacts.trajectory_path, trajectory)
        write_json_file(artifacts.meta_path, meta)
        return relative_to_root(self.paths.root, artifact_dir)

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
                    refreshable=False,
                    snapshot=True,
                    status="ok",
                )
                self.log_refresh(
                    source_key,
                    "ok",
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

    def active_report(self, config: ToolConfig | None = None) -> dict[str, Any]:
        annotation_config = self.annotation_config(config)
        rows = self.conn.execute(
            """
            SELECT *
            FROM peval_py_sources
            WHERE active = 1 AND artifact_dir IS NOT NULL
            ORDER BY created_at_ms ASC, source_key ASC
            """
        ).fetchall()
        if not rows:
            return empty_report("serve")
        stored = [self.read_trial_artifacts(dict(row)) for row in rows]
        trajectories = [item["trajectory"] for item in stored]
        metas = uniquify_trial_keys(
            [
                meta_with_source_alias(item["meta"], row["source_alias"])
                for row, item in zip(rows, stored, strict=True)
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
                rows,
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
    ) -> list[dict[str, Any]]:
        where = []
        params: list[Any] = []
        if source_keys:
            where.append(
                "source_key IN (" + ",".join("?" for _ in source_keys) + ")"
            )
            params.extend(source_keys)
        if active_only:
            where.append("active = 1")
        sql = "SELECT * FROM peval_py_sources"
        if where:
            sql += " WHERE " + " AND ".join(where)
        sql += " ORDER BY created_at_ms ASC, source_key ASC"
        return [dict(row) for row in self.conn.execute(sql, params).fetchall()]

    def source_payload(self) -> list[dict[str, Any]]:
        rows = self.conn.execute(
            """
            SELECT *
            FROM peval_py_sources
            ORDER BY created_at_ms ASC, source_key ASC
            """
        ).fetchall()
        payload: list[dict[str, Any]] = []
        for row in rows:
            item = dict(row)
            trajectory = {}
            meta = {}
            artifact_dir = item.get("artifact_dir")
            if artifact_dir:
                artifacts = self.read_trial_artifacts(item)
                trajectory = artifacts["trajectory"]
                meta = artifacts["meta"]
            item["refreshable"] = bool(item["refreshable"])
            item["active"] = bool(item["active"])
            item["snapshot"] = bool(item["snapshot"])
            item["trial_key"] = optional_str(
                meta.get("trial_key") or trajectory.get("trajectory_id")
            )
            item["trial_session_id"] = optional_str(trajectory.get("session_id"))
            item["last_turn_finished_at_ms"] = optional_int(meta.get("finished_at_ms"))
            payload.append(item)
        return payload

    def read_trial_artifacts(self, row: dict[str, Any]) -> dict[str, dict[str, Any]]:
        artifact_dir = row.get("artifact_dir")
        if not artifact_dir:
            raise ValueError(f"trial has no artifact directory: {row.get('source_key')}")
        artifacts = trial_artifacts(self.resolve_artifact_dir(str(artifact_dir)))
        return {
            "trajectory": read_json_object(artifacts.trajectory_path),
            "meta": read_json_object(artifacts.meta_path),
        }

    def resolve_artifact_dir(self, artifact_dir: str) -> Path:
        path = Path(artifact_dir)
        if not path.is_absolute():
            path = self.paths.root / path
        return path.resolve()

    def set_source_active(self, source_key: str, active: bool) -> None:
        cursor = self.conn.execute(
            """
            UPDATE peval_py_sources
            SET active = ?, updated_at_ms = ?
            WHERE source_key = ?
            """,
            (1 if active else 0, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def set_source_alias(self, source_key: str, alias: str | None) -> None:
        cursor = self.conn.execute(
            """
            UPDATE peval_py_sources
            SET source_alias = ?, updated_at_ms = ?
            WHERE source_key = ?
            """,
            (alias or None, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def delete_source(self, source_key: str) -> None:
        row = self.conn.execute(
            """
            SELECT source_key, artifact_dir
            FROM peval_py_sources
            WHERE source_key = ?
            """,
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        artifact_dir = row["artifact_dir"]
        self.conn.execute(
            "DELETE FROM peval_py_refresh_log WHERE source_key = ?",
            (source_key,),
        )
        self.conn.execute("DELETE FROM peval_py_sources WHERE source_key = ?", (source_key,))
        self.conn.commit()
        if artifact_dir:
            remove_artifact_dir(
                self.paths.root,
                self.resolve_artifact_dir(str(artifact_dir)),
            )

    def save_source_notes(
        self,
        source_key: str,
        markdown: str,
        config: ToolConfig,
    ) -> None:
        row = self.conn.execute(
            """
            SELECT *
            FROM peval_py_sources
            WHERE source_key = ?
            """,
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        source = dict(row)
        if not source.get("refreshable") or source.get("snapshot"):
            raise ValueError("notes.md can only be saved for refreshable sources")
        if not source.get("artifact_dir"):
            raise ValueError("notes.md requires a persisted Trial cell")
        write_note_file(
            self.resolve_artifact_dir(str(source["artifact_dir"])) / "notes.md",
            self.paths.root,
            markdown,
        )
        self.refresh_source(source, config)

    def log_refresh(
        self,
        source_key: str,
        status: str,
        warning_count: int,
        error: str | None,
        timestamp: int,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO peval_py_refresh_log
            (source_key, status, warning_count, error, refreshed_at_ms)
            VALUES (?, ?, ?, ?, ?)
            """,
            (source_key, status, warning_count, error, timestamp),
        )
        self.conn.execute(
            """
            DELETE FROM peval_py_refresh_log
            WHERE id NOT IN (
                SELECT id
                FROM peval_py_refresh_log
                ORDER BY refreshed_at_ms DESC, id DESC
                LIMIT ?
            )
            """,
            (REFRESH_LOG_LIMIT,),
        )


def open_workspace_state(root: str | None = None) -> ServeStateStore:
    resolved = resolve_workspace_root(root)
    return ServeStateStore(workspace_paths(resolved))


def open_workspace_state_readonly(root: str | None = None) -> ServeStateStore:
    resolved = resolve_workspace_root(root)
    return ServeStateStore(
        workspace_paths(resolved),
        initialize=False,
        readonly=True,
    )


def load_serve_inputs(
    args: Any,
    adapter_assignments: AdapterAssignments,
    config: ToolConfig | None = None,
) -> LoadedInputs:
    return load_inputs(args, adapter_assignments, require_sources=False, config=config)


def loaded_session_from_source(source: dict[str, Any]) -> LoadedSession:
    return LoadedSession(
        records=None,
        input_label=str(source["label"]),
        adapter_id=str(source["adapter"]),
        input_path=source.get("input_path") or source.get("db_path"),
        db_path=source.get("db_path"),
        session_hint=source.get("session_id"),
        agent_name=source.get("agent_name"),
        agent_version=source.get("agent_version"),
        model=source.get("model"),
        source_alias=source.get("source_alias"),
        source_kind=str(source["kind"]),
    )


def source_row_for_session(session: LoadedSession) -> dict[str, Any]:
    return {
        "kind": session.source_kind,
        "adapter": session.adapter_id,
        "label": session.input_label,
        "input_path": normalized_optional_path(session.input_path),
        "db_path": normalized_optional_path(session.db_path),
        "session_id": session.session_hint,
        "source_alias": session.source_alias,
        "agent_name": session.agent_name,
        "agent_version": session.agent_version,
        "model": session.model,
    }


def trial_payload_from_report(
    report: dict[str, Any],
) -> tuple[dict[str, Any], dict[str, Any]]:
    trajectories = report.get("trajectory") or []
    metas = report.get("trajectory_meta") or []
    if len(trajectories) != 1 or len(metas) != 1:
        raise ValueError("source refresh must produce exactly one Trial")
    trajectory = trajectories[0]
    meta = metas[0]
    if not isinstance(trajectory, dict) or not isinstance(meta, dict):
        raise ValueError("source refresh produced non-object Trial data")
    return trajectory, meta


def source_key_for_trial(
    eval_slug: str,
    source: dict[str, Any],
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> str:
    payload = trial_cell_components(
        eval_slug=eval_slug,
        source=source,
        trajectory=trajectory,
        meta=meta,
    )
    return "cell_" + hashlib.sha256(
        json.dumps(payload, sort_keys=True).encode("utf-8")
    ).hexdigest()[:20]


def trial_cell_components(
    *,
    eval_slug: str,
    source: dict[str, Any],
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> dict[str, str]:
    agent = trajectory.get("agent")
    trajectory_agent = agent.get("name") if isinstance(agent, dict) else None
    return {
        "eval_slug": artifact_segment(eval_slug, DEFAULT_ANALYSIS_EVAL_SLUG),
        "agent_id": artifact_segment(
            source.get("agent_name")
            or trajectory_agent
            or meta.get("adapter")
            or source.get("adapter"),
            "unknown-agent",
        ),
        "session_id": artifact_segment(
            trajectory.get("session_id")
            or source.get("session_id")
            or meta.get("trial_key"),
            "unknown-session",
        ),
        "cell_key": required_artifact_segment(meta.get("trial_key"), "trial_key"),
    }


def normalized_optional_path(path: str | None) -> str | None:
    if not path:
        return None
    return str(Path(path).expanduser().resolve())


def workspace_analysis_eval_slug(paths: WorkspacePaths) -> str:
    if not paths.config_path.is_file():
        return DEFAULT_ANALYSIS_EVAL_SLUG
    try:
        data = tomllib.loads(paths.config_path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError:
        return DEFAULT_ANALYSIS_EVAL_SLUG
    value = data.get("analysis_eval_slug")
    return artifact_segment(value, DEFAULT_ANALYSIS_EVAL_SLUG)


def trial_cell_dir(
    root: Path,
    *,
    eval_slug: str,
    source: dict[str, Any],
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> Path:
    components = trial_cell_components(
        eval_slug=eval_slug,
        source=source,
        trajectory=trajectory,
        meta=meta,
    )
    return (
        root
        / "runs"
        / components["eval_slug"]
        / components["agent_id"]
        / components["session_id"]
        / components["cell_key"]
    )


def trial_artifacts(artifact_dir: Path) -> TrialArtifacts:
    agent_dir = artifact_dir / AGENT_DIR
    return TrialArtifacts(
        trajectory_path=agent_dir / TRAJECTORY_FILENAME,
        meta_path=agent_dir / TRAJECTORY_META_FILENAME,
    )


def artifact_segment(value: Any, fallback: str) -> str:
    text = str(value or "").strip()
    safe = "".join(
        char if char.isalnum() or char in {"-", "_", "."} else "_"
        for char in text
    ).strip("._")
    return safe or fallback


def required_artifact_segment(value: Any, label: str) -> str:
    text = str(value or "").strip()
    safe = "".join(
        char if char.isalnum() or char in {"-", "_", "."} else "_"
        for char in text
    ).strip("._")
    if not safe:
        raise ValueError(f"{label} is required for Trial cell artifacts")
    return safe


def relative_to_root(root: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return str(path.resolve())


def write_json_file(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    tmp.replace(path)


def write_text_file(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(value, encoding="utf-8")
    tmp.replace(path)


def read_json_object(path: Path) -> dict[str, Any]:
    return json_object(path.read_text(encoding="utf-8"), str(path))


def json_object(value: str, label: str) -> dict[str, Any]:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse {label}: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValueError(f"{label} must contain a JSON object")
    return parsed


def remove_artifact_dir(root: Path, artifact_dir: Path) -> None:
    resolved_root = root.resolve()
    resolved_artifact = artifact_dir.resolve()
    if resolved_artifact == resolved_root or resolved_root not in resolved_artifact.parents:
        raise ValueError(
            f"refusing to remove artifact directory outside workspace: {artifact_dir}"
        )
    if resolved_artifact.is_dir():
        shutil.rmtree(resolved_artifact)


def matching_annotation_items(
    annotations: dict[str, Any],
    key: str,
    trial_key: str,
) -> list[dict[str, Any]]:
    return [
        deepcopy(item)
        for item in annotations.get(key) or []
        if isinstance(item, dict) and str(item.get("trial_key") or "") == trial_key
    ]


def merged_note_markdown(notes: list[dict[str, Any]]) -> str | None:
    sections: list[str] = []
    for index, note in enumerate(notes, start=1):
        markdown = optional_str(note.get("markdown"))
        if not markdown or not markdown.strip():
            continue
        label = optional_str(note.get("label"))
        source = optional_str(note.get("source"))
        if label or source:
            heading = label or f"Note {index}"
            lines = [f"## {heading}"]
            if source:
                lines.extend(["", f"Source: {source}"])
            lines.extend(["", markdown.strip()])
            sections.append("\n".join(lines))
        else:
            sections.append(markdown.strip())
    if not sections:
        return None
    return "\n\n".join(sections) + "\n"


def merged_analysis_json(analyses: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not analyses:
        return None
    summaries = [
        summary.strip()
        for summary in (optional_str(item.get("summary")) for item in analyses)
        if summary and summary.strip()
    ]
    payload: dict[str, Any] = {
        "summary": "\n\n".join(summaries) if summaries else "",
        "items": deepcopy(analyses),
    }
    for key in MERGEABLE_ANALYSIS_LIST_FIELDS:
        values: list[Any] = []
        for item in analyses:
            value = item.get(key)
            if isinstance(value, list) and value:
                values.extend(deepcopy(value))
        if values:
            payload[key] = values
    for source_key, target_key in (
        ("analysis_status", "status"),
        ("subject", "subject"),
        ("analysis_metrics", "metrics"),
        ("confidence", "confidence"),
    ):
        value = unique_analysis_value(analyses, source_key)
        if value is not None:
            payload[target_key] = value
    return payload


def unique_analysis_value(items: list[dict[str, Any]], key: str) -> Any:
    values: list[Any] = []
    for item in items:
        if key not in item:
            continue
        value = item[key]
        if key == "analysis_status":
            if isinstance(value, str) and value.strip():
                values.append(value)
        elif key in {"subject", "analysis_metrics"}:
            if isinstance(value, dict) and value:
                values.append(deepcopy(value))
        elif key == "confidence":
            if isinstance(value, str) and value.strip():
                values.append(value)
            elif (
                isinstance(value, (int, float))
                and not isinstance(value, bool)
                and math.isfinite(float(value))
            ):
                values.append(value)
    unique_values: list[Any] = []
    for value in values:
        if not any(value == existing for existing in unique_values):
            unique_values.append(value)
    if len(unique_values) == 1:
        return unique_values[0]
    return None


def merged_analysis_markdown(analyses: list[dict[str, Any]]) -> str | None:
    sections = [
        markdown.strip()
        for markdown in (optional_str(item.get("md_report")) for item in analyses)
        if markdown and markdown.strip()
    ]
    if not sections:
        return None
    return "\n\n".join(sections) + "\n"


def is_report_json(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("trajectory"), list)
        and isinstance(value.get("trajectory_meta"), list)
        and value.get("schema_version") is not None
    )


def uniquify_trial_keys(metas: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: dict[str, int] = {}
    out: list[dict[str, Any]] = []
    for meta in metas:
        copy = dict(meta)
        base = str(copy.get("trial_key") or "trial")
        count = seen.get(base, 0) + 1
        seen[base] = count
        if count > 1:
            copy["trial_key"] = f"{base}:{count}"
        out.append(copy)
    return out


def meta_with_source_alias(meta: dict[str, Any], alias: Any) -> dict[str, Any]:
    if alias:
        copy = dict(meta)
        copy["source_alias"] = str(alias)
        return copy
    if "source_alias" in meta:
        copy = dict(meta)
        copy.pop("source_alias", None)
        return copy
    return meta


def source_report_with_current_annotations(
    source: dict[str, Any],
    trajectory: dict[str, Any],
    meta: dict[str, Any],
    config: ToolConfig | None,
) -> dict[str, Any]:
    if config is None:
        return {}

    trial_key = str(meta.get("trial_key") or "")
    session_id = (
        optional_str(trajectory.get("session_id"))
        or source.get("session_id")
        or optional_str(meta.get("trial_key"))
    )
    agent_id = annotation_agent_id(source, trajectory)
    current_note = cached_note_report(
        workspace_root=config.workspace_root,
        eval_slug=config.analysis_eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        trial_key=trial_key,
    )
    current_analysis = cached_analysis_report(
        workspace_root=config.workspace_root,
        eval_slug=config.analysis_eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        trial_key=trial_key,
    )
    notes: list[dict[str, Any]] = []
    if current_note is not None:
        notes.append(current_note)

    next_annotations: dict[str, Any] = {
        "report_notes": [],
        "notes": notes,
    }
    if current_analysis is not None:
        next_annotations["analysis"] = [current_analysis]

    if (
        next_annotations["report_notes"]
        or next_annotations["notes"]
        or next_annotations.get("analysis")
    ):
        return {"annotations": next_annotations}
    return {}


def annotation_agent_id(source: dict[str, Any], trajectory: dict[str, Any]) -> str | None:
    agent = trajectory.get("agent")
    trajectory_agent = agent.get("name") if isinstance(agent, dict) else None
    return (
        optional_str(source.get("agent_name"))
        or optional_str(trajectory_agent)
        or optional_str(source.get("adapter"))
    )


def parsed_object(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if not isinstance(value, str):
        return {}
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def optional_str(value: Any) -> str | None:
    return None if value is None else str(value)


def optional_int(value: Any) -> int | None:
    if value is None:
        return None
    return int(value)
