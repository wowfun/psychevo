from __future__ import annotations

from collections.abc import Mapping
from typing import Any, cast

JSON_SAFE_INTEGER_MAX = 9_007_199_254_740_991
JSON_SAFE_INTEGER_MIN = -JSON_SAFE_INTEGER_MAX


def wire_object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


def wire_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ValueError(f"{label} must be an array")
    return value


def wire_string_tuple(value: object, label: str) -> tuple[str, ...]:
    items = wire_list(value, label)
    if not all(isinstance(item, str) for item in items):
        raise ValueError(f"{label} must contain only strings")
    return cast(tuple[str, ...], tuple(items))


def wire_present(value: Mapping[str, object], key: str, label: str) -> object:
    if key not in value:
        raise ValueError(f"{label} requires {key}")
    return value[key]


def wire_string(value: Mapping[str, object], key: str, label: str) -> str:
    field = wire_present(value, key, label)
    if not isinstance(field, str):
        raise ValueError(f"{label} {key} must be a string")
    return field


def wire_optional_string(
    value: Mapping[str, object], key: str, label: str
) -> str | None:
    field = value.get(key)
    if field is not None and not isinstance(field, str):
        raise ValueError(f"{label} {key} must be a string or null")
    return field


def wire_nullable_string(
    value: Mapping[str, object], key: str, label: str
) -> str | None:
    field = wire_present(value, key, label)
    if field is not None and not isinstance(field, str):
        raise ValueError(f"{label} {key} must be a string or null")
    return field


def wire_int(value: Mapping[str, object], key: str, label: str) -> int:
    field = wire_present(value, key, label)
    if type(field) is not int:
        raise ValueError(f"{label} {key} must be an integer")
    return field


def wire_optional_int(
    value: Mapping[str, object], key: str, label: str
) -> int | None:
    field = value.get(key)
    if field is not None and type(field) is not int:
        raise ValueError(f"{label} {key} must be an integer or null")
    return field


def wire_json_safe_int(
    value: Mapping[str, object],
    key: str,
    label: str,
    *,
    minimum: int = JSON_SAFE_INTEGER_MIN,
) -> int:
    field = wire_int(value, key, label)
    if not minimum <= field <= JSON_SAFE_INTEGER_MAX:
        raise ValueError(f"{label} {key} must be a JSON-safe integer")
    return field


def wire_optional_json_safe_int(
    value: Mapping[str, object],
    key: str,
    label: str,
    *,
    minimum: int = JSON_SAFE_INTEGER_MIN,
) -> int | None:
    field = wire_optional_int(value, key, label)
    if field is not None and not minimum <= field <= JSON_SAFE_INTEGER_MAX:
        raise ValueError(f"{label} {key} must be a JSON-safe integer")
    return field


def wire_nullable_json_safe_int(
    value: Mapping[str, object],
    key: str,
    label: str,
    *,
    minimum: int = JSON_SAFE_INTEGER_MIN,
) -> int | None:
    field = wire_present(value, key, label)
    if field is not None and type(field) is not int:
        raise ValueError(f"{label} {key} must be an integer or null")
    if field is not None and not minimum <= field <= JSON_SAFE_INTEGER_MAX:
        raise ValueError(f"{label} {key} must be a JSON-safe integer")
    return field


def wire_bool(value: Mapping[str, object], key: str, label: str) -> bool:
    field = wire_present(value, key, label)
    if type(field) is not bool:
        raise ValueError(f"{label} {key} must be a boolean")
    return field


def wire_enum(
    value: Mapping[str, object],
    key: str,
    label: str,
    choices: frozenset[str],
) -> str:
    field = wire_string(value, key, label)
    if field not in choices:
        raise ValueError(f"{label} {key} is invalid: {field}")
    return field
