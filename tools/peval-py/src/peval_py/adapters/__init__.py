from __future__ import annotations

from collections.abc import Callable
from importlib.metadata import EntryPoint, entry_points
from typing import Any

from peval_py.adapters.base import Adapter
from peval_py.adapters.hermes import HermesAdapter
from peval_py.adapters.opencode import OpencodeAdapter
from peval_py.adapters.psychevo import PsychevoAdapter

ENTRY_POINT_GROUP = "peval_py.adapters"

AdapterFactory = Callable[[], Adapter]

BUILTIN_ADAPTERS: dict[str, AdapterFactory] = {
    "psychevo": PsychevoAdapter,
    "opencode": OpencodeAdapter,
    "hermes": HermesAdapter,
}


def adapter_for(adapter: str) -> Adapter:
    adapter_id = normalize_adapter_id(adapter)
    registry = adapter_factories()
    factory = registry.get(adapter_id)
    if factory is None:
        available = ", ".join(sorted(registry)) or "<none>"
        raise ValueError(
            f"unsupported adapter: {adapter}; available adapters: {available}"
        )
    instance = factory()
    require_adapter_protocol(adapter_id, instance)
    return instance


def available_adapter_ids() -> list[str]:
    return sorted(adapter_factories())


def adapter_factories() -> dict[str, AdapterFactory]:
    factories = dict(BUILTIN_ADAPTERS)
    for entry_point in adapter_entry_points():
        adapter_id = normalize_adapter_id(entry_point.name)
        if adapter_id in factories:
            raise ValueError(f"duplicate adapter id: {adapter_id}")
        factories[adapter_id] = entry_point_factory(adapter_id, entry_point)
    return factories


def adapter_entry_points() -> list[EntryPoint]:
    discovered = entry_points()
    if hasattr(discovered, "select"):
        return list(discovered.select(group=ENTRY_POINT_GROUP))
    return list(discovered.get(ENTRY_POINT_GROUP, []))


def entry_point_factory(adapter_id: str, entry_point: EntryPoint) -> AdapterFactory:
    def factory() -> Adapter:
        return coerce_adapter(adapter_id, entry_point.load())

    return factory


def coerce_adapter(adapter_id: str, value: Any) -> Adapter:
    if isinstance(value, type):
        value = value()
    elif callable(value) and not (
        callable(getattr(value, "convert", None))
        or callable(getattr(value, "convert_path", None))
    ):
        value = value()
    require_adapter_protocol(adapter_id, value)
    return value


def require_adapter_protocol(adapter_id: str, value: Any) -> None:
    if not (
        callable(getattr(value, "convert", None))
        or callable(getattr(value, "convert_path", None))
    ):
        raise ValueError(
            f"adapter {adapter_id} must define convert(records, config) "
            "or convert_path(path, config)"
        )


def normalize_adapter_id(adapter: object) -> str:
    text = str(adapter or "").strip().lower()
    if not text:
        raise ValueError("adapter id is required")
    return text
