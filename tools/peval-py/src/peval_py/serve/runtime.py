from __future__ import annotations

from argparse import Namespace
from threading import Event, Lock, Thread
from typing import Any

from peval_py.config import ToolConfig
from peval_py.inputs import AdapterAssignments
from peval_py.report import empty_report
from peval_py.serve.reporting import mutation_payload
from peval_py.serve.sources import load_serve_inputs
from peval_py.state import ServeStateStore


class ServeRuntime:
    def __init__(
        self,
        store: ServeStateStore,
        config: ToolConfig,
        *,
        initialize_snapshot: bool = True,
    ) -> None:
        self.store = store
        self.config = config
        self._lock = Lock()
        self._ready = Event()
        self._ready.set()
        self._thread: Thread | None = None
        self._loading = False
        self._load_error: str | None = None
        self._snapshot = self.empty_envelope(loading=False)
        if initialize_snapshot:
            self.refresh_snapshot()

    def start_initial_load(
        self,
        args: Namespace,
        adapter_assignments: AdapterAssignments,
    ) -> None:
        with self._lock:
            if self._thread is not None:
                return
            self._loading = True
            self._load_error = None
            self._snapshot = self.empty_envelope(loading=True)
            self._ready.clear()
            self._thread = Thread(
                target=self._run_initial_load,
                args=(args, adapter_assignments),
                daemon=True,
            )
            self._thread.start()

    def _run_initial_load(
        self,
        args: Namespace,
        adapter_assignments: AdapterAssignments,
    ) -> None:
        try:
            loaded_inputs = load_serve_inputs(args, adapter_assignments, self.config)
            self.store.import_loaded_sources(loaded_inputs, self.config)
            self.store.sync_artifact_sources(self.config)
            snapshot = self.build_envelope()
            error = None
        except Exception as exc:  # noqa: BLE001 - background startup boundary.
            snapshot = self.empty_envelope(loading=False, error=str(exc))
            error = str(exc)
        with self._lock:
            self._snapshot = snapshot
            self._loading = False
            self._load_error = error
            self._ready.set()

    def wait_until_ready(self, timeout: float | None = None) -> bool:
        return self._ready.wait(timeout)

    def ensure_ready(self) -> None:
        self._ready.wait()
        with self._lock:
            error = self._load_error
        if error:
            raise ValueError(error)

    def is_loading(self) -> bool:
        with self._lock:
            return self._loading

    def set_config(self, config: ToolConfig) -> None:
        with self._lock:
            self.config = config

    def source_envelope(self, *, refresh: bool = False) -> dict[str, Any]:
        if refresh and not self.is_loading():
            with self._lock:
                if self._load_error:
                    return dict(self._snapshot)
            self.ensure_ready()
            return self.refresh_snapshot()
        with self._lock:
            return dict(self._snapshot)

    def report(
        self,
        *,
        source_keys: list[str] | None = None,
        source_state: str = "active",
    ) -> dict[str, Any]:
        if self.is_loading():
            return empty_report("serve")
        self.ensure_ready()
        return self.store.active_report(
            self.config,
            source_keys=source_keys,
            source_state=source_state,
        )

    def refresh_snapshot(self, *, source_state: str = "active") -> dict[str, Any]:
        payload = self.build_envelope(source_state=source_state)
        with self._lock:
            self._snapshot = payload
            self._loading = False
            self._load_error = None
            self._ready.set()
        return payload

    def mutation_envelope(
        self,
        *,
        source_key: str | None = None,
        source_state: str = "active",
    ) -> dict[str, Any]:
        self.ensure_ready()
        payload = self.build_envelope(
            source_key=source_key,
            source_state=source_state,
        )
        with self._lock:
            self._snapshot = payload
        return payload

    def build_envelope(
        self,
        *,
        source_key: str | None = None,
        source_state: str = "active",
    ) -> dict[str, Any]:
        payload = mutation_payload(
            self.store,
            self.config,
            source_key=source_key,
            source_state=source_state,
        )
        payload["loading"] = False
        return payload

    def empty_envelope(
        self,
        *,
        loading: bool,
        error: str | None = None,
    ) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "sources": [],
            "report": empty_report("serve"),
            "report_source_key": None,
            "report_source_state": "active",
            "loading": loading,
        }
        if error:
            payload["error"] = error
        return payload
