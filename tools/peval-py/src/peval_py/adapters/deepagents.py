from __future__ import annotations

import json
import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from peval_py.adapters.base import SessionInfo
from peval_py.adapters.common import CommonMessageAdapter
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord

DEEPAGENTS_TIMING_SOURCE = "deepagents_session_json"


class DeepagentsAdapter(CommonMessageAdapter):
    agent_id = "deepagents"
    default_agent_name = "deepagents"

    def convert_path(self, path: str, config: ToolConfig):
        source = Path(path).expanduser()
        if not source.exists():
            raise ValueError(f"Deepagents session file not found: {source}")
        records = read_deepagents_json(str(source))
        return self.convert(records, config)

    def list_sessions(self, path: str) -> list[SessionInfo]:
        source = Path(path).expanduser()
        if source.is_file():
            return [SessionInfo(session_id=source.stem, name=None)]
        sessions: list[SessionInfo] = []
        for f in sorted(source.glob("*.json")):
            sessions.append(SessionInfo(session_id=f.stem, name=None))
        return sessions


def read_deepagents_json(path: str) -> list[MessageRecord]:
    raw = Path(path).read_text(encoding="utf-8")
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError("Deepagents session file is not a JSON object")

    session_id = data.get("session_id") or Path(path).stem
    started_at_ms = iso_to_ms(data.get("started_at"))
    query = str(data.get("query") or "")
    llm_steps = data.get("llm_steps") or []
    tool_steps = data.get("tool_steps") or []

    tool_results_by_round = _index_tool_results(tool_steps)
    tool_call_ids_by_round = _index_tool_call_ids(tool_steps)

    system_prompts = _extract_system_prompts(llm_steps)
    records: list[MessageRecord] = []
    seq = 1

    for sp_text in system_prompts:
        records.append(
            MessageRecord(
                message={
                    "role": "system",
                    "content": sp_text,
                    "timestamp_ms": started_at_ms,
                },
                metadata={
                    "session_id": session_id,
                    "source": DEEPAGENTS_TIMING_SOURCE,
                },
                session_seq=seq,
                source_session_id=session_id,
            )
        )
        seq += 1

    if query:
        records.append(
            MessageRecord(
                message={
                    "role": "user",
                    "content": query,
                    "timestamp_ms": started_at_ms,
                },
                metadata={
                    "session_id": session_id,
                    "source": DEEPAGENTS_TIMING_SOURCE,
                },
                session_seq=seq,
                source_session_id=session_id,
            )
        )
        seq += 1

    for llm_step in llm_steps:
        round_num = llm_step.get("round", 0)
        output_text = str(llm_step.get("output_text") or "")
        reasoning = llm_step.get("reasoning_content") or ""
        if not isinstance(reasoning, str):
            reasoning = str(reasoning)
        elapsed_ms = _int_or_none(llm_step.get("elapsed_ms"))
        timestamp_ms = iso_to_ms(llm_step.get("timestamp"))
        started_ms = timestamp_ms - elapsed_ms if timestamp_ms and elapsed_ms else None

        tool_calls_raw = llm_step.get("tool_calls") or []
        round_call_ids = tool_call_ids_by_round.get(round_num, [])
        content: list[dict[str, Any]] = []
        if output_text:
            content.append({"type": "text", "text": output_text})

        for tc_index, tc in enumerate(tool_calls_raw):
            call_id = round_call_ids[tc_index] if tc_index < len(round_call_ids) else f"da-r{round_num}-{tc_index}"
            name = str(tc.get("name") or "tool")
            args = tc.get("args")
            if not isinstance(args, dict):
                args = _parse_json_args(tc.get("args_full")) or {}
            content.append(
                {
                    "type": "tool_call",
                    "id": call_id,
                    "name": name,
                    "arguments": args,
                }
            )

        message: dict[str, Any] = {
            "role": "assistant",
            "content": content if content else output_text,
            "timestamp_ms": timestamp_ms,
        }
        if reasoning:
            message["reasoning_content"] = reasoning

        metadata: dict[str, Any] = {
            "session_id": session_id,
            "source": DEEPAGENTS_TIMING_SOURCE,
        }
        if started_ms is not None:
            metadata["started_at_ms"] = started_ms
            metadata["started_at_ms_source"] = DEEPAGENTS_TIMING_SOURCE
        if timestamp_ms is not None:
            metadata["finished_at_ms"] = timestamp_ms
            metadata["finished_at_ms_source"] = DEEPAGENTS_TIMING_SOURCE
        if elapsed_ms is not None:
            metadata["elapsed_ms"] = elapsed_ms
            metadata["elapsed_ms_source"] = DEEPAGENTS_TIMING_SOURCE

        usage = _usage_from_step(llm_step)

        records.append(
            MessageRecord(
                message=message,
                usage=usage or None,
                metadata=metadata,
                session_seq=seq,
                source_session_id=session_id,
            )
        )
        seq += 1

        round_tool_results = tool_results_by_round.get(round_num, [])
        for tc_index, tc in enumerate(tool_calls_raw):
            call_id = round_call_ids[tc_index] if tc_index < len(round_call_ids) else f"da-r{round_num}-{tc_index}"
            tool_name = str(tc.get("name") or "tool")
            tool_result = _find_tool_result(round_tool_results, tc_index, tool_name)
            result_text = tool_result.get("result_text", "") if tool_result else ""
            parsed = _parse_result_text(result_text)
            tool_content = parsed.content if parsed.content is not None else result_text
            tool_error_text = tool_result.get("error", "") if tool_result else ""
            tool_elapsed_ms = _int_or_none(tool_result.get("elapsed_ms")) if tool_result else None
            tool_timestamp_ms = iso_to_ms(tool_result.get("timestamp")) if tool_result else None
            tool_started_ms = (
                tool_timestamp_ms - tool_elapsed_ms
                if tool_timestamp_ms and tool_elapsed_ms
                else None
            )
            is_error = bool(tool_error_text)

            result_metadata: dict[str, Any] = {
                "session_id": session_id,
                "source": DEEPAGENTS_TIMING_SOURCE,
            }
            if tool_started_ms is not None:
                result_metadata["started_at_ms"] = tool_started_ms
                result_metadata["started_at_ms_source"] = DEEPAGENTS_TIMING_SOURCE
            if tool_timestamp_ms is not None:
                result_metadata["finished_at_ms"] = tool_timestamp_ms
                result_metadata["finished_at_ms_source"] = DEEPAGENTS_TIMING_SOURCE
            if tool_elapsed_ms is not None:
                result_metadata["elapsed_ms"] = tool_elapsed_ms
                result_metadata["elapsed_ms_source"] = DEEPAGENTS_TIMING_SOURCE

            records.append(
                MessageRecord(
                    message={
                        "role": "tool_result",
                        "tool_call_id": call_id,
                        "tool_name": tool_name,
                        "content": tool_content,
                        "is_error": is_error,
                        "timestamp_ms": tool_timestamp_ms,
                    },
                    metadata=result_metadata,
                    session_seq=seq,
                    source_session_id=session_id,
                )
            )
            seq += 1

    return records


def _extract_system_prompts(llm_steps: list[dict[str, Any]]) -> list[str]:
    if not llm_steps:
        return []
    prompt_text = llm_steps[0].get("prompt_text") or ""
    if not isinstance(prompt_text, str) or not prompt_text:
        return []
    seen: set[int] = set()
    prompts: list[str] = []
    for m in re.finditer(r"\[system\]", prompt_text):
        start = m.end()
        next_section = _find_next_section(prompt_text, start)
        text = prompt_text[start:next_section].strip().rstrip("-").strip()
        h = hash(text)
        if h not in seen:
            seen.add(h)
            prompts.append(text)
    return prompts


def _find_next_section(text: str, from_pos: int) -> int:
    pattern = re.compile(r"\[(?:system|human|user|ai|tool|tool_calls)")
    m = pattern.search(text, from_pos)
    return m.start() if m else len(text)


def _index_tool_results(tool_steps: list[dict[str, Any]]) -> dict[int, list[dict[str, Any]]]:
    by_round: dict[int, list[dict[str, Any]]] = {}
    for ts in tool_steps:
        r = ts.get("round", 0)
        by_round.setdefault(r, []).append(ts)
    return by_round


def _index_tool_call_ids(tool_steps: list[dict[str, Any]]) -> dict[int, list[str]]:
    by_round: dict[int, list[str]] = {}
    for ts in tool_steps:
        r = ts.get("round", 0)
        parsed = _parse_result_text(ts.get("result_text") or "")
        call_id = parsed.tool_call_id or f"da-r{r}-{len(by_round.setdefault(r, []))}"
        by_round.setdefault(r, []).append(call_id)
    return by_round


_RESULT_CONTENT_PATTERN = re.compile(r"^content='(.*)' name='", re.DOTALL)
_RESULT_TOOL_CALL_ID_PATTERN = re.compile(r"tool_call_id='([^']+)'")


@dataclass
class _ParsedResultText:
    content: str | None = None
    tool_call_id: str | None = None


def _parse_result_text(result_text: str) -> _ParsedResultText:
    if not result_text:
        return _ParsedResultText()
    content_match = _RESULT_CONTENT_PATTERN.match(result_text)
    content = _decode_escape_sequences(content_match.group(1)) if content_match else None
    id_match = _RESULT_TOOL_CALL_ID_PATTERN.search(result_text)
    tool_call_id = id_match.group(1) if id_match else None
    return _ParsedResultText(content=content, tool_call_id=tool_call_id)


_ESCAPE_SEQUENCES = [
    ("\\n", "\n"),
    ("\\t", "\t"),
    ("\\r", "\r"),
    ("\\\\", "\\"),
]


def _decode_escape_sequences(text: str) -> str:
    for escaped, actual in _ESCAPE_SEQUENCES:
        text = text.replace(escaped, actual)
    return text


def _find_tool_result(
    round_results: list[dict[str, Any]],
    index: int,
    tool_name: str,
) -> dict[str, Any] | None:
    if index < len(round_results):
        return round_results[index]
    for r in round_results:
        if r.get("tool") == tool_name:
            return r
    return round_results[0] if round_results else None


def _usage_from_step(step: dict[str, Any]) -> dict[str, Any]:
    usage: dict[str, Any] = {}
    for key, target in [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_tokens"),
        ("total_tokens", "total_tokens"),
        ("reasoning_tokens", "reasoning_tokens"),
    ]:
        value = _int_or_none(step.get(key))
        if value is not None:
            usage[target] = value
    return usage


def _parse_json_args(value: Any) -> dict[str, Any] | None:
    if isinstance(value, dict):
        return value
    if isinstance(value, str) and value:
        try:
            parsed = json.loads(value)
            return parsed if isinstance(parsed, dict) else None
        except json.JSONDecodeError:
            return None
    return None


def iso_to_ms(value: Any) -> int | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    try:
        dt = datetime.fromisoformat(text)
        return int(dt.timestamp() * 1000)
    except (ValueError, OSError):
        return None


def _int_or_none(value: Any) -> int | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
