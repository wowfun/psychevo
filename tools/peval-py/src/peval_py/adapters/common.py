from __future__ import annotations

import json
from typing import Any

from peval_py.adapters.base import (
    ConversionResult,
    ObservationMeta,
    StepMeta,
    ToolMeta,
    timestamp_fallback_allowed,
    timestamp_fallback_duration_ms,
)
from peval_py.config import ToolConfig
from peval_py.redaction import redact_value
from peval_py.sources import MessageRecord


class CommonMessageAdapter:
    agent_id = "common"
    default_agent_name = "agent"

    def convert(self, records: list[MessageRecord], config: ToolConfig) -> ConversionResult:
        steps: list[dict[str, Any]] = []
        meta: list[StepMeta] = []
        pending_tools: dict[str, tuple[dict[str, Any], StepMeta, ToolMeta]] = {}
        warnings: list[str] = []
        unmapped = 0
        next_step_id = 1
        for record in records:
            role = message_role(record.message)
            timestamp = message_timestamp(record.message)
            if role in {"tool", "tool_result"}:
                observation, observation_meta = self.tool_result_observation(
                    record, config, timestamp
                )
                call_id = observation_meta.source_call_id
                if call_id and call_id in pending_tools:
                    atif_step, step_meta, tool_meta = pending_tools[call_id]
                    attach_observation(
                        atif_step,
                        step_meta,
                        tool_meta,
                        observation,
                        observation_meta,
                        record,
                    )
                    continue
                warnings.append(f"unmatched tool result: {call_id or '<missing>'}")
                atif_step, step_meta = self.tool_result_step_from_observation(
                    observation, observation_meta, next_step_id, timestamp
                )
                steps.append(atif_step)
                meta.append(step_meta)
                next_step_id += 1
                continue

            step = self.step_from_record(record, next_step_id, config)
            if step is None:
                unmapped += 1
                warnings.append(
                    f"unmapped message role: {record.message.get('role', '<missing>')}"
                )
                continue
            atif_step, step_meta = step
            steps.append(atif_step)
            meta.append(step_meta)
            for tool_meta in step_meta.tool_calls:
                pending_tools[tool_meta.tool_call_id] = (atif_step, step_meta, tool_meta)
            next_step_id += 1

        started = first_timestamp(meta)
        finished = last_timestamp(meta)
        final_metrics = final_metrics_from_records(records, steps)
        trajectory_id = config.trajectory_id or "session:t001"
        trajectory = {
            "schema_version": "ATIF-v1.7",
            "trajectory_id": trajectory_id,
            "agent": {
                "name": config.agent_name or self.default_agent_name,
                "version": config.agent_version,
            },
            "steps": steps,
            "final_metrics": final_metrics,
        }
        session_id = first_metadata_value(records, "session_id") or first_source_session_id(
            records
        )
        if session_id:
            trajectory["session_id"] = str(session_id)
        model_name = config.model or first_model(records)
        if model_name:
            trajectory["agent"]["model_name"] = model_name
        return ConversionResult(
            trajectory=trajectory,
            steps_meta=meta,
            warnings=warnings,
            total_events=len(records),
            unmapped_events=unmapped,
            started_at_ms=started,
            finished_at_ms=finished,
            timestamp_semantics=first_metadata_value(records, "timestamp_semantics"),
        )

    def step_from_record(
        self, record: MessageRecord, step_id: int, config: ToolConfig
    ) -> tuple[dict[str, Any], StepMeta] | None:
        message = record.message
        role = message_role(message)
        timestamp = message_timestamp(message)
        if role in {"system", "system_message", "system_prompt"}:
            text, truncated = bounded_text(message_text(message), config)
            return build_step(
                step_id,
                "system",
                text,
                timestamp,
                config,
                truncated=truncated,
            )
        if role in {"user", "human"}:
            text, truncated = bounded_text(message_text(message), config)
            return build_step(
                step_id,
                "user",
                text,
                timestamp,
                config,
                truncated=truncated,
            )
        if role in {"assistant", "agent"}:
            return self.assistant_step(record, step_id, config, timestamp)
        if role in {"tool", "tool_result"}:
            return self.tool_result_step(record, step_id, config, timestamp)
        return None

    def assistant_step(
        self,
        record: MessageRecord,
        step_id: int,
        config: ToolConfig,
        timestamp: int | None,
    ) -> tuple[dict[str, Any], StepMeta]:
        message = record.message
        text, truncated = bounded_text(message_text(message), config)
        reasoning_text = reasoning_from_message(message)
        if reasoning_text:
            reasoning_text, reasoning_truncated = bounded_text(reasoning_text, config)
            truncated = truncated or reasoning_truncated
        tool_calls = []
        tool_meta = []
        model_timestamp = metadata_started_at_ms(record) or timestamp
        duration = metadata_elapsed_ms(record)
        duration_source = metadata_elapsed_ms_source(record)
        for call in tool_calls_from_message(message):
            call_id = str(call.get("id") or call.get("tool_call_id") or "tool-call")
            name = str(call.get("name") or call.get("function_name") or call.get("tool") or "tool")
            args = call.get("arguments")
            if not isinstance(args, dict):
                args = parse_json_object(call.get("arguments_json")) or {}
            if config.redact:
                args = redact_value(args)
            tool_calls.append(
                {
                    "tool_call_id": call_id,
                    "function_name": name,
                    "arguments": args,
                }
            )
            tool_meta.append(
                ToolMeta(
                    tool_call_id=call_id,
                    status="pending",
                    title=name,
                    timestamp_ms=timestamp,
                )
            )
        step = {
            "step_id": step_id,
            "source": "agent",
            "message": redact_value(text) if config.redact else text,
        }
        if reasoning_text:
            step["reasoning_content"] = (
                redact_value(reasoning_text) if config.redact else reasoning_text
            )
        if tool_calls:
            step["tool_calls"] = tool_calls
        metrics = metrics_from_record(record)
        if metrics:
            step["metrics"] = metrics
        return step, StepMeta(
            step_id=step_id,
            source="agent",
            tool_calls=tool_meta,
            timestamp_ms=model_timestamp,
            duration_ms=duration,
            duration_source=duration_source,
            truncated=truncated,
        )

    def tool_result_step(
        self,
        record: MessageRecord,
        step_id: int,
        config: ToolConfig,
        timestamp: int | None,
    ) -> tuple[dict[str, Any], StepMeta]:
        observation, observation_meta = self.tool_result_observation(
            record, config, timestamp
        )
        return self.tool_result_step_from_observation(
            observation, observation_meta, step_id, timestamp
        )

    def tool_result_observation(
        self,
        record: MessageRecord,
        config: ToolConfig,
        timestamp: int | None,
    ) -> tuple[dict[str, Any], ObservationMeta]:
        message = record.message
        call_id = message.get("tool_call_id") or message.get("id")
        tool_name = str(message.get("tool_name") or message.get("name") or "tool")
        content = tool_result_content(message)
        content, truncated = bounded_value(content, config)
        is_error = bool(message.get("is_error") or message.get("error"))
        source_call_id = str(call_id) if call_id else None
        result = {"source_call_id": source_call_id} if source_call_id else {}
        result["content"] = redact_value(content) if config.redact else content
        finished_at_ms = metadata_finished_at_ms(record) or timestamp
        return result, ObservationMeta(
            source_call_id=source_call_id,
            status="error" if is_error else "completed",
            title=tool_name,
            timestamp_ms=finished_at_ms,
            tool_error=is_error,
            truncated=truncated,
        )

    def tool_result_step_from_observation(
        self,
        observation: dict[str, Any],
        observation_meta: ObservationMeta,
        step_id: int,
        timestamp: int | None,
    ) -> tuple[dict[str, Any], StepMeta]:
        step = {
            "step_id": step_id,
            "source": "agent",
            "message": "",
            "observation": {"results": [observation]},
        }
        return step, StepMeta(
            step_id=step_id,
            source="agent",
            observations=[observation_meta],
            tool_error=observation_meta.tool_error,
            timestamp_ms=timestamp,
            truncated=observation_meta.truncated,
        )


def attach_observation(
    step: dict[str, Any],
    step_meta: StepMeta,
    tool_meta: ToolMeta,
    observation: dict[str, Any],
    observation_meta: ObservationMeta,
    record: MessageRecord,
) -> None:
    observation_block = step.setdefault("observation", {})
    results = observation_block.setdefault("results", [])
    results.append(observation)
    step_meta.observations.append(observation_meta)
    step_meta.tool_error = step_meta.tool_error or observation_meta.tool_error
    step_meta.truncated = step_meta.truncated or observation_meta.truncated
    if observation_meta.title and (not tool_meta.title or tool_meta.title == "tool"):
        tool_meta.title = observation_meta.title
    if observation_meta.tool_error:
        tool_meta.status = "error"
    elif tool_meta.status != "error":
        tool_meta.status = observation_meta.status or "completed"
    explicit_start_ms = metadata_started_at_ms(record)
    if explicit_start_ms is not None:
        tool_meta.timestamp_ms = explicit_start_ms
    execution_duration_ms, execution_duration_source = tool_execution_duration(
        record,
        tool_meta.timestamp_ms,
        observation_meta.timestamp_ms,
    )
    if execution_duration_ms is not None:
        if explicit_start_ms is None and observation_meta.timestamp_ms is not None:
            tool_meta.timestamp_ms = max(
                0,
                observation_meta.timestamp_ms - execution_duration_ms,
            )
        tool_meta.execution_duration_ms = execution_duration_ms
        tool_meta.execution_duration_source = execution_duration_source


def build_step(
    step_id: int,
    source: str,
    text: str,
    timestamp: int | None,
    config: ToolConfig,
    truncated: bool = False,
) -> tuple[dict[str, Any], StepMeta]:
    value = redact_value(text) if config.redact else text
    return {
        "step_id": step_id,
        "source": source,
        "message": value,
    }, StepMeta(
        step_id=step_id,
        source=source,
        timestamp_ms=timestamp,
        truncated=truncated,
    )


def message_role(message: dict[str, Any]) -> str:
    return str(message.get("role", "")).lower()


def message_timestamp(message: dict[str, Any]) -> int | None:
    value = message.get("timestamp_ms", 0) or 0
    return int(value) or None


def message_text(message: dict[str, Any]) -> str:
    for key in ["message", "text", "prompt", "system_prompt", "output"]:
        value = message.get(key)
        if isinstance(value, str):
            return value
    content = message.get("content")
    return content_text(content)


def content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        texts = []
        for block in content:
            if isinstance(block, str):
                texts.append(block)
            elif isinstance(block, dict):
                block_type = str(block.get("type", "text"))
                if block_type in {"text", "message"} and isinstance(block.get("text"), str):
                    texts.append(block["text"])
                elif isinstance(block.get("content"), str):
                    texts.append(block["content"])
                elif block_type in {"image_url", "local_image"}:
                    texts.append(f"[{block_type}]")
        return "\n".join(texts)
    if isinstance(content, dict):
        text = content.get("text") or content.get("content")
        if isinstance(text, str):
            return text
    return ""


def tool_calls_from_message(message: dict[str, Any]) -> list[dict[str, Any]]:
    calls: list[dict[str, Any]] = []
    direct = message.get("tool_calls")
    if isinstance(direct, list):
        calls.extend(item for item in direct if isinstance(item, dict))
    for block in message.get("content", []):
        if isinstance(block, dict) and str(block.get("type", "")).lower() in {
            "tool_call",
            "tool_calls",
        }:
            calls.append(block)
    return calls


def reasoning_from_message(message: dict[str, Any]) -> str | None:
    direct = first_string(message, ["reasoning", "reasoning_content"])
    blocks = []
    for block in message.get("content", []):
        if (
            isinstance(block, dict)
            and str(block.get("type", "")).lower() == "reasoning"
            and isinstance(block.get("text"), str)
        ):
            blocks.append(block["text"])
    if direct and blocks:
        return "\n\n".join([direct, *blocks])
    if direct:
        return direct
    if blocks:
        return "\n\n".join(blocks)
    return None


def tool_result_content(message: dict[str, Any]) -> Any:
    for key in ["result", "output", "content", "message"]:
        if key in message:
            return message[key]
    return message


def tool_execution_duration(
    record: MessageRecord,
    assistant_timestamp_ms: int | None,
    observation_timestamp_ms: int | None,
) -> tuple[int | None, str | None]:
    elapsed = metadata_elapsed_ms(record)
    if elapsed is not None:
        return max(0, elapsed), metadata_elapsed_ms_source(record) or "message_metadata"
    if not timestamp_fallback_allowed(record_timestamp_semantics(record)):
        return None, None
    fallback = timestamp_fallback_duration_ms(assistant_timestamp_ms, observation_timestamp_ms)
    if fallback is not None:
        return fallback, "event_timestamps"
    return None, None


def record_timestamp_semantics(record: MessageRecord) -> str | None:
    for source in [
        record.metadata,
        as_dict(record.message.get("metadata")),
        record.message,
    ]:
        if isinstance(source, dict) and source.get("timestamp_semantics") is not None:
            return str(source["timestamp_semantics"])
    return None


def metadata_elapsed_ms(record: MessageRecord) -> int | None:
    for source in [record.metadata, as_dict(record.message.get("metadata"))]:
        if isinstance(source, dict):
            value = numeric_value(source.get("elapsed_ms"))
            if value is not None:
                return int(value)
    return None


def metadata_elapsed_ms_source(record: MessageRecord) -> str | None:
    for source in [record.metadata, as_dict(record.message.get("metadata"))]:
        if isinstance(source, dict) and isinstance(source.get("elapsed_ms_source"), str):
            return str(source["elapsed_ms_source"])
    return None


def metadata_started_at_ms(record: MessageRecord) -> int | None:
    return metadata_time_ms(record, "started_at_ms")


def metadata_finished_at_ms(record: MessageRecord) -> int | None:
    return metadata_time_ms(record, "finished_at_ms")


def metadata_time_ms(record: MessageRecord, key: str) -> int | None:
    for source in [record.metadata, as_dict(record.message.get("metadata"))]:
        if isinstance(source, dict):
            value = numeric_value(source.get(key))
            if value is not None:
                return int(value)
    return None


def metrics_from_record(record: MessageRecord) -> dict[str, Any]:
    metrics: dict[str, Any] = {}
    usage = record.usage or as_dict(record.message.get("usage")) or {}
    accounting = record.accounting or {}
    prompt = first_number(usage, ["prompt_tokens", "input_tokens", "total_prompt_tokens"])
    completion = first_number(
        usage, ["completion_tokens", "output_tokens", "total_completion_tokens"]
    )
    cached = first_number(
        usage, ["cached_tokens", "cache_read_tokens", "cache_write_tokens"]
    )
    cost = first_number(usage, ["cost_usd", "total_cost_usd"])
    if prompt is None:
        prompt = first_number(accounting, ["context_input_tokens", "billable_input_tokens"])
    if completion is None:
        completion = first_number(accounting, ["billable_output_tokens"])
    if cached is None:
        cached = sum_present(
            first_number(accounting, ["cache_read_tokens"]),
            first_number(accounting, ["cache_write_tokens"]),
        )
    if cost is None and accounting.get("estimated_cost_nanodollars") is not None:
        cost = float(accounting["estimated_cost_nanodollars"]) / 1_000_000_000
    if prompt is not None:
        metrics["prompt_tokens"] = int(prompt)
    if completion is not None:
        metrics["completion_tokens"] = int(completion)
    if cached is not None:
        metrics["cached_tokens"] = int(cached)
    if cost is not None:
        metrics["cost_usd"] = float(cost)
    extra = metrics_extra(usage, accounting)
    if extra:
        metrics["extra"] = extra
    return metrics


def final_metrics_from_records(
    records: list[MessageRecord], steps: list[dict[str, Any]]
) -> dict[str, Any]:
    prompt = completion = cached = 0
    cost = 0.0
    have_prompt = have_completion = have_cached = have_cost = False
    usage_totals: dict[str, float] = {}
    accounting_totals: dict[str, float] = {}
    pricing_source: str | None = None
    tool_calls = 0
    tool_errors = 0
    turns = 0
    for record in records:
        role = str(record.message.get("role", "")).lower()
        if role in {"assistant", "agent"}:
            turns += 1
            tool_calls += len(tool_calls_from_message(record.message))
        if role in {"tool", "tool_result"} and (
            record.message.get("is_error") or record.message.get("error")
        ):
            tool_errors += 1
        metrics = metrics_from_record(record)
        metric_extra = as_dict(metrics.get("extra")) or {}
        aggregate_usage(usage_totals, metric_extra.get("usage"))
        pricing_source = aggregate_accounting(
            accounting_totals,
            metric_extra.get("accounting"),
            pricing_source,
        )
        if "prompt_tokens" in metrics:
            have_prompt = True
            prompt += int(metrics["prompt_tokens"])
        if "completion_tokens" in metrics:
            have_completion = True
            completion += int(metrics["completion_tokens"])
        if "cached_tokens" in metrics:
            have_cached = True
            cached += int(metrics["cached_tokens"])
        if "cost_usd" in metrics:
            have_cost = True
            cost += float(metrics["cost_usd"])
    final: dict[str, Any] = {"total_steps": len(steps)}
    if have_prompt:
        final["total_prompt_tokens"] = prompt
    if have_completion:
        final["total_completion_tokens"] = completion
    if have_cached:
        final["total_cached_tokens"] = cached
    if have_cost:
        final["total_cost_usd"] = round(cost, 12)
    extra: dict[str, Any] = {
        "total_tool_calls": tool_calls,
        "total_tool_errors": tool_errors,
    }
    if turns:
        extra["total_turns"] = turns
    usage = finalized_totals(usage_totals)
    if usage:
        extra["usage"] = usage
    accounting = finalized_totals(accounting_totals)
    if pricing_source:
        accounting["pricing_source"] = pricing_source
    if accounting:
        extra["accounting"] = accounting
    if extra:
        final["extra"] = extra
    return final


def metrics_extra(usage: dict[str, Any], accounting: dict[str, Any]) -> dict[str, Any]:
    extra: dict[str, Any] = {}
    if usage:
        extra["usage"] = usage
    if accounting:
        extra["accounting"] = accounting
    return extra


def aggregate_usage(totals: dict[str, float], usage: Any) -> None:
    if not isinstance(usage, dict):
        return
    add_total(
        totals,
        "input_tokens",
        first_number(usage, ["input_tokens", "prompt_tokens", "total_prompt_tokens"]),
    )
    add_total(
        totals,
        "output_tokens",
        first_number(usage, ["output_tokens", "completion_tokens", "total_completion_tokens"]),
    )
    add_total(
        totals,
        "cache_read_tokens",
        first_number(usage, ["cache_read_tokens", "cached_tokens"]),
    )
    add_total(totals, "cache_write_tokens", first_number(usage, ["cache_write_tokens"]))
    add_total(totals, "reasoning_tokens", first_number(usage, ["reasoning_tokens"]))


def aggregate_accounting(
    totals: dict[str, float], accounting: Any, pricing_source: str | None
) -> str | None:
    if not isinstance(accounting, dict):
        return pricing_source
    for key in [
        "billable_input_tokens",
        "billable_output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
    ]:
        add_total(totals, key, numeric_value(accounting.get(key)))
    source = accounting.get("pricing_source")
    return str(source) if source else pricing_source


def add_total(totals: dict[str, float], key: str, value: float | None) -> None:
    if value is not None:
        totals[key] = totals.get(key, 0.0) + value


def finalized_totals(totals: dict[str, float]) -> dict[str, int | float]:
    values: dict[str, int | float] = {}
    for key, value in totals.items():
        values[key] = int(value) if float(value).is_integer() else value
    return values


def bounded_text(value: str, config: ToolConfig) -> tuple[str, bool]:
    if len(value) <= config.max_content_chars:
        return value, False
    return value[: config.max_content_chars], True


def bounded_value(value: Any, config: ToolConfig) -> tuple[Any, bool]:
    if isinstance(value, str):
        return bounded_text(value, config)
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True)
    if len(raw) <= config.max_content_chars:
        return value, False
    return raw[: config.max_content_chars], True


def first_timestamp(meta: list[StepMeta]) -> int | None:
    timestamps = [timestamp for step in meta for timestamp in step_timestamps(step)]
    return min(timestamps) if timestamps else None


def last_timestamp(meta: list[StepMeta]) -> int | None:
    timestamps = [timestamp for step in meta for timestamp in step_timestamps(step)]
    return max(timestamps) if timestamps else None


def step_timestamps(step: StepMeta) -> list[int]:
    timestamps: list[int] = []
    if step.timestamp_ms is not None:
        timestamps.append(step.timestamp_ms)
        if step.duration_ms is not None:
            timestamps.append(step.timestamp_ms + max(0, step.duration_ms))
    timestamps.extend(
        tool.timestamp_ms for tool in step.tool_calls if tool.timestamp_ms is not None
    )
    timestamps.extend(
        tool.timestamp_ms + max(0, tool.execution_duration_ms)
        for tool in step.tool_calls
        if tool.timestamp_ms is not None and tool.execution_duration_ms is not None
    )
    timestamps.extend(
        observation.timestamp_ms
        for observation in step.observations
        if observation.timestamp_ms is not None
    )
    return timestamps


def first_metadata_value(records: list[MessageRecord], key: str) -> Any:
    for record in records:
        for source in [
            record.metadata,
            as_dict(record.message.get("metadata")),
            record.message,
        ]:
            if isinstance(source, dict) and source.get(key) is not None:
                return source[key]
    return None


def first_source_session_id(records: list[MessageRecord]) -> str | None:
    for record in records:
        if record.source_session_id:
            return record.source_session_id
    return None


def first_model(records: list[MessageRecord]) -> str | None:
    for record in records:
        for source in [record.message, record.metadata]:
            if isinstance(source, dict):
                value = source.get("model") or source.get("model_name")
                if isinstance(value, str) and value:
                    return value
    return None


def first_string(value: dict[str, Any], keys: list[str]) -> str | None:
    for key in keys:
        item = value.get(key)
        if isinstance(item, str) and item:
            return item
    return None


def first_number(value: dict[str, Any], keys: list[str]) -> float | None:
    for key in keys:
        number = numeric_value(value.get(key))
        if number is not None:
            return number
    return None


def numeric_value(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int | float):
        return float(value)
    if isinstance(value, str) and value:
        try:
            return float(value)
        except ValueError:
            return None
    return None


def sum_present(*values: float | None) -> float | None:
    present = [value for value in values if value is not None]
    if not present:
        return None
    return float(sum(present))


def as_dict(value: Any) -> dict[str, Any] | None:
    return value if isinstance(value, dict) else None


def parse_json_object(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None
