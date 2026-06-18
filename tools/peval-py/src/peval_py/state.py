from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import time
import tomllib
from copy import deepcopy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.analysis import cached_analysis_report, cached_note_report, save_cell_note
from peval_py.atif import convert_atif_trajectory, is_atif_trajectory
from peval_py.config import ToolConfig, config_for_adapter
from peval_py.inputs import AdapterAssignments, LoadedInputs, LoadedSession, load_inputs
from peval_py.pipeline import report_session_for_loaded
from peval_py.report import (
    ReportSession,
    build_multi_report,
    build_report_from_snapshots,
    empty_report,
    token_total,
    trial_wall_duration_ms,
)
from peval_py.sources import read_jsonl_text

PEVAL_PY_CONFIG = "peval-py.toml"
PEVAL_ROOT_ENV = "PEVAL_ROOT"
STATE_SCHEMA_VERSION = 2
UPLOAD_LIMIT_BYTES = 20 * 1024 * 1024
REFRESH_LOG_LIMIT = 200


@dataclass(frozen=True)
class WorkspacePaths:
    root: Path
    config_path: Path
    state_db_path: Path


@dataclass(frozen=True)
class SourceSnapshot:
    source_key: str
    source: dict[str, Any]


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
        config_path.write_text('state_db = "state.db"\n', encoding="utf-8")
    return WorkspacePaths(root=root, config_path=config_path, state_db_path=state_db_path)


class ServeStateStore:
    def __init__(self, paths: WorkspacePaths) -> None:
        self.paths = paths
        self.paths.state_db_path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(self.paths.state_db_path, check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self.migrate()

    def close(self) -> None:
        self.conn.close()

    def migrate(self) -> None:
        self.conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS peval_py_schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at_ms INTEGER NOT NULL
            );
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
                refreshable INTEGER NOT NULL,
                active INTEGER NOT NULL,
                snapshot INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_status TEXT,
                last_error TEXT,
                last_refreshed_at_ms INTEGER
            );
            CREATE TABLE IF NOT EXISTS peval_py_trials (
                source_key TEXT PRIMARY KEY,
                trial_key TEXT NOT NULL,
                session_id TEXT,
                adapter TEXT,
                status TEXT,
                duration_ms INTEGER,
                wall_duration_ms INTEGER,
                turns INTEGER,
                tools INTEGER,
                tokens INTEGER,
                cost_usd REAL,
                trajectory_json TEXT NOT NULL,
                meta_json TEXT NOT NULL,
                report_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                FOREIGN KEY(source_key) REFERENCES peval_py_sources(source_key)
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
        self.ensure_column("peval_py_sources", "source_alias", "TEXT")
        self.conn.execute(
            """
            INSERT OR IGNORE INTO peval_py_schema_migrations(version, applied_at_ms)
            VALUES (?, ?)
            """,
            (STATE_SCHEMA_VERSION, now_ms()),
        )
        self.conn.commit()

    def ensure_column(self, table: str, column: str, definition: str) -> None:
        columns = {
            str(row["name"])
            for row in self.conn.execute(f"PRAGMA table_info({table})").fetchall()
        }
        if column not in columns:
            self.conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {definition}")

    def upsert_loaded_sources(
        self,
        loaded_inputs: LoadedInputs,
        config: ToolConfig,
    ) -> list[str]:
        keys = []
        for session in loaded_inputs.sessions:
            keys.append(self.upsert_loaded_source(session, config))
        return keys

    def upsert_loaded_source(
        self,
        session: LoadedSession,
        config: ToolConfig,
        *,
        commit: bool = True,
        timestamp: int | None = None,
    ) -> str:
        key = source_key_for_session(session)
        timestamp = timestamp if timestamp is not None else now_ms()
        self.conn.execute(
            """
            INSERT INTO peval_py_sources
            (source_key, kind, adapter, label, input_path, db_path, session_id,
             source_alias, agent_name, agent_version, model,
             refreshable, active, snapshot, created_at_ms, updated_at_ms, last_status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 1, 0, ?, ?, 'pending')
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
                refreshable = 1,
                snapshot = 0,
                active = 1,
                updated_at_ms = excluded.updated_at_ms
            """,
            (
                key,
                session.source_kind,
                session.adapter_id,
                session.input_label,
                normalized_optional_path(session.input_path),
                normalized_optional_path(session.db_path),
                session.session_hint,
                session.source_alias,
                session.agent_name,
                session.agent_version,
                session.model,
                timestamp,
                timestamp,
            ),
        )
        if commit:
            self.conn.commit()
        return key

    def import_loaded_sources(
        self,
        loaded_inputs: LoadedInputs,
        config: ToolConfig,
    ) -> list[str]:
        prepared: list[tuple[str, LoadedSession, dict[str, Any], int]] = []
        for session in loaded_inputs.sessions:
            source_key = source_key_for_session(session)
            report_session = report_session_for_loaded(session, config)
            report = build_multi_report([report_session], config, [])
            warnings = report["trajectory_meta"][0].get("warnings") or []
            prepared.append((source_key, session, report, len(warnings)))

        timestamp = now_ms()
        try:
            for source_key, session, report, warning_count in prepared:
                self.upsert_loaded_source(
                    session,
                    config,
                    commit=False,
                    timestamp=timestamp,
                )
                self.store_report_for_source(source_key, report)
                self.conn.execute(
                    """
                    UPDATE peval_py_sources
                    SET last_status = ?, last_error = NULL,
                        last_refreshed_at_ms = ?, updated_at_ms = ?
                    WHERE source_key = ?
                    """,
                    ("ok", timestamp, timestamp, source_key),
                )
                self.log_refresh(source_key, "ok", warning_count, None, timestamp)
            self.conn.commit()
        except Exception:
            self.conn.rollback()
            raise
        return [source_key for source_key, _, _, _ in prepared]

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
            self.store_report_for_source(source_key, report)
            warnings = report["trajectory_meta"][0].get("warnings") or []
            self.conn.execute(
                """
                UPDATE peval_py_sources
                SET last_status = ?, last_error = NULL, last_refreshed_at_ms = ?,
                    updated_at_ms = ?
                WHERE source_key = ?
                """,
                ("ok", timestamp, timestamp, source_key),
            )
            self.log_refresh(source_key, "ok", len(warnings), None, timestamp)
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

    def store_report_for_source(self, source_key: str, report: dict[str, Any]) -> None:
        trajectories = report.get("trajectory") or []
        metas = report.get("trajectory_meta") or []
        if len(trajectories) != 1 or len(metas) != 1:
            raise ValueError("source refresh must produce exactly one Trial")
        self.store_trial(source_key, trajectories[0], metas[0], report)

    def store_trial(
        self,
        source_key: str,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
        report: dict[str, Any],
    ) -> None:
        metrics = trajectory.get("final_metrics") if isinstance(trajectory, dict) else {}
        if not isinstance(metrics, dict):
            metrics = {}
        self.conn.execute(
            """
            INSERT INTO peval_py_trials
            (source_key, trial_key, session_id, adapter, status, duration_ms,
             wall_duration_ms, turns, tools, tokens, cost_usd, trajectory_json,
             meta_json, report_json, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_key) DO UPDATE SET
                trial_key = excluded.trial_key,
                session_id = excluded.session_id,
                adapter = excluded.adapter,
                status = excluded.status,
                duration_ms = excluded.duration_ms,
                wall_duration_ms = excluded.wall_duration_ms,
                turns = excluded.turns,
                tools = excluded.tools,
                tokens = excluded.tokens,
                cost_usd = excluded.cost_usd,
                trajectory_json = excluded.trajectory_json,
                meta_json = excluded.meta_json,
                report_json = excluded.report_json,
                updated_at_ms = excluded.updated_at_ms
            """,
            (
                source_key,
                str(meta.get("trial_key") or trajectory.get("trajectory_id") or source_key),
                optional_str(trajectory.get("session_id")),
                optional_str(meta.get("adapter")),
                optional_str(meta.get("status")),
                optional_int(meta.get("duration_ms")),
                optional_int(trial_wall_duration_ms(meta)),
                optional_int(metrics.get("total_turns")),
                optional_int(metrics.get("total_tool_calls")),
                optional_int(token_total(metrics)),
                optional_float(metrics.get("total_cost_usd")),
                json.dumps(trajectory, ensure_ascii=False),
                json.dumps(meta, ensure_ascii=False),
                json.dumps(report, ensure_ascii=False),
                now_ms(),
            ),
        )

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
            return self.ingest_report_snapshot(parsed_json, label)
        if isinstance(parsed_json, dict) and is_atif_trajectory(parsed_json):
            conversion = convert_atif_trajectory(parsed_json)
            report = build_multi_report(
                [ReportSession(conversion=conversion, input_label=label, adapter_id="atif")],
                config_for_adapter(config, "atif"),
                [],
            )
            return self.ingest_report_snapshot(report, label, adapter="atif")
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
        return self.ingest_report_snapshot(report, label, adapter=source_config.adapter)

    def ingest_report_snapshot(
        self,
        report: dict[str, Any],
        label: str,
        *,
        adapter: str | None = None,
    ) -> list[str]:
        trajectories = report.get("trajectory")
        metas = report.get("trajectory_meta")
        if not isinstance(trajectories, list) or not isinstance(metas, list):
            raise ValueError("report JSON snapshot must contain trajectory and trajectory_meta arrays")
        if len(trajectories) != len(metas):
            raise ValueError("report JSON snapshot trajectory/meta counts differ")
        keys: list[str] = []
        digest = content_digest(report)
        for index, (trajectory, meta) in enumerate(zip(trajectories, metas, strict=True), start=1):
            if not isinstance(trajectory, dict) or not isinstance(meta, dict):
                raise ValueError("report JSON snapshot contains non-object Trial data")
            source_key = snapshot_source_key(label, digest, meta.get("trial_key"), index)
            source_label = (
                f"{label}:{trajectory.get('session_id') or meta.get('trial_key') or index}"
            )
            timestamp = now_ms()
            self.conn.execute(
                """
                INSERT INTO peval_py_sources
                (source_key, kind, adapter, label, input_path, db_path, session_id,
                 source_alias, agent_name, agent_version, model,
                 refreshable, active, snapshot, created_at_ms, updated_at_ms,
                 last_status, last_refreshed_at_ms)
                VALUES (?, 'snapshot', ?, ?, NULL, NULL, ?, NULL, NULL, NULL, NULL,
                        0, 1, 1, ?, ?, 'ok', ?)
                ON CONFLICT(source_key) DO UPDATE SET
                    adapter = excluded.adapter,
                    label = excluded.label,
                    session_id = excluded.session_id,
                    source_alias = COALESCE(excluded.source_alias, peval_py_sources.source_alias),
                    active = 1,
                    updated_at_ms = excluded.updated_at_ms,
                    last_status = 'ok',
                    last_error = NULL,
                    last_refreshed_at_ms = excluded.last_refreshed_at_ms
                """,
                (
                    source_key,
                    adapter or optional_str(meta.get("adapter")) or "snapshot",
                    source_label,
                    optional_str(trajectory.get("session_id")),
                    timestamp,
                    timestamp,
                    timestamp,
                ),
            )
            single_report = build_report_from_snapshots([trajectory], [meta], input_label=label)
            self.store_trial(source_key, trajectory, meta, single_report)
            self.log_refresh(source_key, "ok", len(meta.get("warnings") or []), None, timestamp)
            keys.append(source_key)
        self.conn.commit()
        return keys

    def active_report(self, config: ToolConfig | None = None) -> dict[str, Any]:
        rows = self.conn.execute(
            """
            SELECT s.*, t.trial_key AS stored_trial_key,
                   t.trajectory_json, t.meta_json, t.report_json
            FROM peval_py_sources s
            JOIN peval_py_trials t ON t.source_key = s.source_key
            WHERE s.active = 1
            ORDER BY s.created_at_ms ASC, s.source_key ASC
            """
        ).fetchall()
        if not rows:
            return empty_report("serve")
        trajectories = [json.loads(row["trajectory_json"]) for row in rows]
        metas = uniquify_trial_keys(
            [
                meta_with_source_alias(json.loads(row["meta_json"]), row["source_alias"])
                for row in rows
            ]
        )
        reports = [
            source_report_with_current_annotations(dict(row), trajectory, meta, config)
            for row, trajectory, meta in zip(rows, trajectories, metas, strict=True)
        ]
        return build_report_from_snapshots(
            trajectories,
            metas,
            input_label="serve",
            source_reports=reports,
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
            SELECT s.*, t.trial_key AS stored_trial_key,
                   t.session_id AS trial_session_id, t.meta_json AS meta_json
            FROM peval_py_sources s
            LEFT JOIN peval_py_trials t ON t.source_key = s.source_key
            ORDER BY s.created_at_ms ASC, s.source_key ASC
            """
        ).fetchall()
        payload: list[dict[str, Any]] = []
        for row in rows:
            item = dict(row)
            meta = parsed_object(item.pop("meta_json", None))
            item["refreshable"] = bool(item["refreshable"])
            item["active"] = bool(item["active"])
            item["snapshot"] = bool(item["snapshot"])
            item["trial_key"] = optional_str(item.pop("stored_trial_key", None))
            item["trial_session_id"] = optional_str(item.get("trial_session_id"))
            item["last_turn_finished_at_ms"] = optional_int(meta.get("finished_at_ms"))
            payload.append(item)
        return payload

    def set_source_active(self, source_key: str, active: bool) -> None:
        cursor = self.conn.execute(
            "UPDATE peval_py_sources SET active = ?, updated_at_ms = ? WHERE source_key = ?",
            (1 if active else 0, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def set_source_alias(self, source_key: str, alias: str | None) -> None:
        cursor = self.conn.execute(
            "UPDATE peval_py_sources SET source_alias = ?, updated_at_ms = ? WHERE source_key = ?",
            (alias or None, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def delete_source(self, source_key: str) -> None:
        exists = self.conn.execute(
            "SELECT 1 FROM peval_py_sources WHERE source_key = ?",
            (source_key,),
        ).fetchone()
        if exists is None:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.execute("DELETE FROM peval_py_trials WHERE source_key = ?", (source_key,))
        self.conn.execute(
            "DELETE FROM peval_py_refresh_log WHERE source_key = ?",
            (source_key,),
        )
        self.conn.execute("DELETE FROM peval_py_sources WHERE source_key = ?", (source_key,))
        self.conn.commit()

    def save_source_notes(
        self,
        source_key: str,
        markdown: str,
        config: ToolConfig,
    ) -> None:
        row = self.conn.execute(
            """
            SELECT s.*, t.trajectory_json AS trajectory_json
            FROM peval_py_sources s
            LEFT JOIN peval_py_trials t ON t.source_key = s.source_key
            WHERE s.source_key = ?
            """,
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        source = dict(row)
        if not source.get("refreshable") or source.get("snapshot"):
            raise ValueError("notes.md can only be saved for refreshable sources")
        trajectory: dict[str, Any] = {}
        if source.get("trajectory_json"):
            try:
                parsed = json.loads(source["trajectory_json"])
            except json.JSONDecodeError:
                parsed = None
            if isinstance(parsed, dict):
                trajectory = parsed
        session_id = optional_str(trajectory.get("session_id")) or source.get("session_id")
        agent_id = source.get("agent_name") or source.get("adapter")
        save_cell_note(
            workspace_root=str(self.paths.root),
            eval_slug=config.analysis_eval_slug,
            agent_id=agent_id,
            session_id=session_id,
            markdown=markdown,
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


def source_key_for_session(session: LoadedSession) -> str:
    payload = {
        "kind": session.source_kind,
        "adapter": session.adapter_id,
        "input_path": normalized_optional_path(session.input_path),
        "db_path": normalized_optional_path(session.db_path),
        "session_id": session.session_hint or "",
    }
    return "src_" + hashlib.sha256(
        json.dumps(payload, sort_keys=True).encode("utf-8")
    ).hexdigest()[:20]


def snapshot_source_key(label: str, digest: str, trial_key: object, index: int) -> str:
    payload = {
        "kind": "snapshot",
        "label": label,
        "digest": digest,
        "trial_key": str(trial_key or index),
        "index": index,
    }
    return "src_" + hashlib.sha256(
        json.dumps(payload, sort_keys=True).encode("utf-8")
    ).hexdigest()[:20]


def normalized_optional_path(path: str | None) -> str | None:
    if not path:
        return None
    return str(Path(path).expanduser().resolve())


def content_digest(value: Any) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()


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
    report = parsed_object(source.get("report_json"))
    if config is None or not bool(source.get("refreshable")) or bool(source.get("snapshot")):
        return report

    trial_key = str(meta.get("trial_key") or "")
    session_id = optional_str(trajectory.get("session_id")) or source.get("session_id")
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
    annotations = parsed_object(report.get("annotations"))
    stored_non_cell_notes = [
        deepcopy(item)
        for item in annotations.get("notes") or []
        if not is_cell_note(item)
    ]
    notes: list[dict[str, Any]] = []
    if current_note is not None:
        notes.append(current_note)
    notes.extend(stored_non_cell_notes)

    next_annotations: dict[str, Any] = {
        "report_notes": deepcopy(annotations.get("report_notes") or []),
        "notes": notes,
    }
    if current_analysis is not None:
        next_annotations["analysis"] = [current_analysis]

    if (
        next_annotations["report_notes"]
        or next_annotations["notes"]
        or next_annotations.get("analysis")
    ):
        report = deepcopy(report)
        report["annotations"] = next_annotations
    else:
        report = deepcopy(report)
        report.pop("annotations", None)
    return report


def annotation_agent_id(source: dict[str, Any], trajectory: dict[str, Any]) -> str | None:
    agent = trajectory.get("agent")
    trajectory_agent = agent.get("name") if isinstance(agent, dict) else None
    return (
        optional_str(source.get("agent_name"))
        or optional_str(trajectory_agent)
        or optional_str(source.get("adapter"))
    )


def is_cell_note(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and value.get("source") == "cell"
        and value.get("label") == "notes.md"
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


def optional_float(value: Any) -> float | None:
    if value is None:
        return None
    return float(value)
