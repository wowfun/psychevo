from __future__ import annotations

from peval_py.adapters.base import Adapter
from peval_py.adapters.hermes import HermesAdapter
from peval_py.adapters.opencode import OpencodeAdapter
from peval_py.adapters.psychevo import PsychevoAdapter


def adapter_for(adapter: str) -> Adapter:
    normalized = adapter.lower()
    if normalized == "psychevo":
        return PsychevoAdapter()
    if normalized == "opencode":
        return OpencodeAdapter()
    if normalized == "hermes":
        return HermesAdapter()
    raise ValueError(f"unsupported adapter: {adapter}")
