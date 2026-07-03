from __future__ import annotations

import json
import os
import sqlite3
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.analysis import write_note_file
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
from peval_py._state.annotations import (
    is_report_json,
    matching_annotation_items,
    meta_with_source_alias,
    merged_analysis_json,
    merged_analysis_markdown,
    merged_note_markdown,
    optional_int,
    optional_str,
    parsed_object,
    source_report_with_current_annotations,
    uniquify_trial_keys,
)
from peval_py._state.artifacts import (
    AGENT_DIR,
    DEFAULT_ANALYSIS_EVAL_SLUG,
    TRAJECTORY_FILENAME,
    TRAJECTORY_META_FILENAME,
    TrialArtifacts,
    normalized_optional_path,
    read_json_object,
    relative_to_root,
    remove_artifact_dir,
    source_key_for_trial,
    trial_artifacts,
    trial_cell_dir,
    workspace_analysis_eval_slug,
    write_json_file,
    write_text_file,
)
from peval_py._state.sources import (
    loaded_session_from_source,
    source_row_for_session,
    trial_payload_from_report,
)

PEVAL_PY_CONFIG = "peval-py.toml"
PEVAL_ROOT_ENV = "PEVAL_ROOT"
STATE_SCHEMA_VERSION = 4
UPLOAD_LIMIT_BYTES = 20 * 1024 * 1024
REFRESH_LOG_LIMIT = 200
SOURCE_STATUS_MISSING = "missing"
SOURCE_STATUS_OK = "ok"


@dataclass(frozen=True)
class WorkspacePaths:
    root: Path
    config_path: Path
    state_db_path: Path


def now_ms() -> int:
    return int(time.time() * 1000)


def trial_summary(
    trajectory: dict[str, Any] | None,
    meta: dict[str, Any] | None,
) -> dict[str, Any]:
    trajectory = trajectory or {}
    meta = meta or {}
    return {
        "trial_key": optional_str(meta.get("trial_key") or trajectory.get("trajectory_id")),
        "trial_session_id": optional_str(trajectory.get("session_id")),
        "last_turn_finished_at_ms": optional_int(meta.get("finished_at_ms")),
    }


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
                trial_key TEXT,
                trial_session_id TEXT,
                last_turn_finished_at_ms INTEGER,
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
        self.ensure_source_columns()
        self.conn.commit()

    def ensure_source_columns(self) -> None:
        existing = {
            str(row["name"])
            for row in self.conn.execute("PRAGMA table_info(peval_py_sources)").fetchall()
        }
        columns = {
            "trial_key": "TEXT",
            "trial_session_id": "TEXT",
            "last_turn_finished_at_ms": "INTEGER",
        }
        for name, sql_type in columns.items():
            if name not in existing:
                self.conn.execute(
                    f"ALTER TABLE peval_py_sources ADD COLUMN {name} {sql_type}"
                )

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
                    trajectory=trajectory,
                    meta=meta,
                    refreshable=True,
                    snapshot=False,
                    status=SOURCE_STATUS_OK,
                )
                self.log_refresh(source_key, SOURCE_STATUS_OK, warning_count, None, timestamp)
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
        trajectory: dict[str, Any] | None = None,
        meta: dict[str, Any] | None = None,
        refreshable: bool,
        snapshot: bool,
        status: str,
        error: str | None = None,
    ) -> None:
        summary = trial_summary(trajectory, meta)
        self.conn.execute(
            """
            INSERT INTO peval_py_sources
            (source_key, kind, adapter, label, input_path, db_path, session_id,
             source_alias, agent_name, agent_version, model,
             artifact_dir, artifact_updated_at_ms,
             trial_key, trial_session_id, last_turn_finished_at_ms,
             refreshable, active, snapshot, created_at_ms, updated_at_ms,
             last_status, last_error, last_refreshed_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)
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
                trial_key = excluded.trial_key,
                trial_session_id = excluded.trial_session_id,
                last_turn_finished_at_ms = excluded.last_turn_finished_at_ms,
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
                summary["trial_key"],
                summary["trial_session_id"],
                summary["last_turn_finished_at_ms"],
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

    def active_report(
        self,
        config: ToolConfig | None = None,
        *,
        source_keys: list[str] | None = None,
    ) -> dict[str, Any]:
        annotation_config = self.annotation_config(config)
        rows = self.source_rows(
            source_keys=source_keys,
            active_only=source_keys is None,
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
                meta_with_source_alias(item["meta"], row.get("source_alias"))
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

    def read_trial_artifacts(self, row: dict[str, Any]) -> dict[str, dict[str, Any]]:
        artifact_dir = row.get("artifact_dir")
        if not artifact_dir:
            raise ValueError(f"trial has no artifact directory: {row.get('source_key')}")
        if self.artifact_missing(row):
            raise ValueError(self.missing_artifact_message(row))
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

    def artifact_missing(self, row: dict[str, Any]) -> bool:
        artifact_dir = row.get("artifact_dir")
        if not artifact_dir:
            return False
        artifacts = trial_artifacts(self.resolve_artifact_dir(str(artifact_dir)))
        return not (artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file())

    def missing_artifact_message(self, row: dict[str, Any]) -> str:
        artifact_dir = row.get("artifact_dir")
        return f"Trial cell artifacts not found: {artifact_dir or row.get('source_key')}"

    def update_source_summary(
        self,
        source_key: str,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
    ) -> None:
        summary = trial_summary(trajectory, meta)
        self.conn.execute(
            """
            UPDATE peval_py_sources
            SET trial_key = ?, trial_session_id = ?, last_turn_finished_at_ms = ?
            WHERE source_key = ?
            """,
            (
                summary["trial_key"],
                summary["trial_session_id"],
                summary["last_turn_finished_at_ms"],
                source_key,
            ),
        )

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

    def discover_trial_cell_dirs(self, eval_slug: str) -> list[Path]:
        run_root = self.paths.root / "runs" / eval_slug
        if not run_root.is_dir():
            return []
        cells: list[Path] = []
        for agent_dir in sorted(run_root.iterdir()):
            if not agent_dir.is_dir():
                continue
            for session_dir in sorted(agent_dir.iterdir()):
                if not session_dir.is_dir():
                    continue
                for cell_dir in sorted(session_dir.iterdir()):
                    if not cell_dir.is_dir():
                        continue
                    artifacts = trial_artifacts(cell_dir)
                    if artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file():
                        cells.append(cell_dir)
        return cells

    def source_row_for_artifact_cell(
        self,
        cell_dir: Path,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
    ) -> dict[str, Any]:
        agent = trajectory.get("agent") if isinstance(trajectory.get("agent"), dict) else {}
        adapter = optional_str(meta.get("adapter") or agent.get("name")) or "artifact"
        return {
            "kind": "trial-artifact",
            "adapter": adapter,
            "label": relative_to_root(self.paths.root, cell_dir),
            "input_path": str(cell_dir.resolve()),
            "db_path": None,
            "session_id": optional_str(trajectory.get("session_id")),
            "source_alias": None,
            "agent_name": optional_str(agent.get("name")),
            "agent_version": None,
            "model": optional_str(agent.get("model_name")),
        }

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
