from __future__ import annotations

from typing import Any

SUPPORTED_LOCALES = ("en", "zh-CN")

_ALIASES = {
    "en": "en",
    "en-us": "en",
    "zh": "zh-CN",
    "zh-cn": "zh-CN",
}


def normalize_locale(value: Any) -> str:
    if value is None:
        return "en"
    text = str(value).strip()
    if not text:
        return "en"
    normalized = _ALIASES.get(text.lower())
    if normalized:
        return normalized
    supported = ", ".join(SUPPORTED_LOCALES)
    raise ValueError(f"unsupported locale: {text}; supported locales: {supported}")


def messages_for(locale: str) -> dict[str, str]:
    return dict(MESSAGES[normalize_locale(locale)])


MESSAGES: dict[str, dict[str, str]] = {
    "en": {
        "title": "Agent Trajectory Report",
        "report_note": "Report note",
        "leaderboard": "Leaderboard",
        "leaderboard_copy": (
            "Each row is one visible session-as-Trial. Numeric cells shade by "
            "column value; rows update the selected Trial."
        ),
        "trajectory_overview": "Trajectory Overview",
        "trajectory_overview_copy": (
            "Rows follow the current Leaderboard order. Nodes align by step index "
            "and show role initials."
        ),
        "duration": "Duration",
        "tokens": "Tokens",
        "tool_calls": "Tool Calls",
        "turns": "Turns",
        "session": "Session",
        "agent": "Agent",
        "model": "Model",
        "result": "Result",
        "status": "status",
        "cost": "Cost",
        "notes": "Notes",
        "sort": "Sort",
        "filter": "Filter",
        "clear": "Clear",
        "selected_count": "selected",
        "step_details": "Step details",
        "close": "Close",
        "no_matching_rows": "No matching rows",
        "selected_trial_trajectory": "selected trial trajectory",
        "run": "Run",
        "selected_session_label": "session",
        "trial": "trial",
        "variant": "variant",
        "agent_model": "agent / model",
        "time": "time",
        "wall_duration": "wall duration",
        "steps_events": "steps/events",
        "system_exposed": "system exposed",
        "reasoning_exposed": "reasoning exposed",
        "yes": "yes",
        "no": "no",
        "score": "score",
        "evaluator": "evaluator",
        "tool_success_total": "tool success / total",
        "no_notes": "No notes.",
        "evidence": "Evidence",
        "usage_breakdown": "Usage Breakdown",
        "input": "input",
        "output": "output",
        "cache_read": "cache read",
        "cache_write": "cache write",
        "reasoning": "reasoning",
        "billable_input": "billable input",
        "billable_output": "billable output",
        "pricing": "pricing",
        "warnings": "Warnings",
        "input_source": "Input Source",
        "status.passed": "passed",
        "status.failed": "failed",
    },
    "zh-CN": {
        "title": "Agent 轨迹报告",
        "report_note": "报告备注",
        "leaderboard": "Leaderboard",
        "leaderboard_copy": "每一行是一条 visible session-as-Trial。数值单元格按列值显示深浅，点击行会更新选中的 Trial。",
        "trajectory_overview": "轨迹概览",
        "trajectory_overview_copy": "行顺序跟随当前 Leaderboard；节点按 step 序号对齐，并显示 role 首字母。",
        "duration": "耗时",
        "tokens": "Token",
        "tool_calls": "Tool Calls",
        "turns": "Turns",
        "session": "Session",
        "agent": "Agent",
        "model": "模型",
        "result": "Result",
        "status": "状态",
        "cost": "费用",
        "notes": "Notes",
        "sort": "排序",
        "filter": "筛选",
        "clear": "清除",
        "selected_count": "已选",
        "step_details": "Step 详情",
        "close": "关闭",
        "no_matching_rows": "没有匹配的行",
        "selected_trial_trajectory": "selected trial trajectory",
        "run": "Run",
        "selected_session_label": "session",
        "trial": "Trial",
        "variant": "variant",
        "agent_model": "Agent / 模型",
        "time": "时间",
        "wall_duration": "总耗时",
        "steps_events": "steps/events",
        "system_exposed": "包含系统提示词",
        "reasoning_exposed": "reasoning exposed",
        "yes": "是",
        "no": "否",
        "score": "分数",
        "evaluator": "evaluator",
        "tool_success_total": "tool success / total",
        "no_notes": "No notes.",
        "evidence": "Evidence",
        "usage_breakdown": "用量明细",
        "input": "输入",
        "output": "输出",
        "cache_read": "cache read",
        "cache_write": "cache write",
        "reasoning": "reasoning",
        "billable_input": "计费输入",
        "billable_output": "计费输出",
        "pricing": "计费来源",
        "warnings": "警告",
        "input_source": "输入来源",
        "status.passed": "通过",
        "status.failed": "失败",
    },
}
