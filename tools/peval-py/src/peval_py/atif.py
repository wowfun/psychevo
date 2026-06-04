from __future__ import annotations

from peval_py.adapters import adapter_for
from peval_py.adapters.base import ConversionResult
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl


def convert_records(records: list[MessageRecord], config: ToolConfig) -> ConversionResult:
    adapter = adapter_for(config.adapter)
    convert = getattr(adapter, "convert", None)
    if not callable(convert):
        raise ValueError(f"adapter {config.adapter} does not support record input")
    return convert(records, config)


def convert_path(path: str, config: ToolConfig) -> ConversionResult:
    adapter = adapter_for(config.adapter)
    adapter_convert_path = getattr(adapter, "convert_path", None)
    if callable(adapter_convert_path):
        return adapter_convert_path(path, config)
    convert = getattr(adapter, "convert", None)
    if not callable(convert):
        raise ValueError(f"adapter {config.adapter} does not support path input")
    return convert(read_jsonl(path), config)
