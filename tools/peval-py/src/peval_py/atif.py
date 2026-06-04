from __future__ import annotations

from peval_py.adapters import adapter_for
from peval_py.adapters.base import ConversionResult
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord


def convert_records(records: list[MessageRecord], config: ToolConfig) -> ConversionResult:
    adapter = adapter_for(config.adapter)
    return adapter.convert(records, config)
