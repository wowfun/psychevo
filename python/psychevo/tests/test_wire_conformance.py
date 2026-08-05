from __future__ import annotations

from dataclasses import fields, is_dataclass
import json
from pathlib import Path
import sys
import unittest
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).parents[1] / "src"))

import psychevo
from psychevo import (
    CompactionResult,
    FilesystemApprovalRequest,
    FilesystemApprovalTarget,
    McpHttpStartupTarget,
    McpStartupApprovalRequest,
    McpStdioStartupTarget,
    PendingInteraction,
    ThreadItem,
    ThreadSnapshot,
    ThreadSummary,
    TurnEvent,
    TurnReceipt,
    TurnResult,
)

_FIXTURE_PATH = (
    Path(__file__).resolve().parents[3]
    / "packages"
    / "protocol"
    / "fixtures"
    / "app-python-wire.json"
)
_CORPUS = json.loads(_FIXTURE_PATH.read_text(encoding="utf-8"))

Decoder = Callable[[object], object]

_DECODERS: dict[str, Decoder] = {
    "PendingInteraction": PendingInteraction.from_wire,
    "CompactionResult": CompactionResult.from_wire,
    "ThreadItem": ThreadItem.from_wire,
    "ThreadSummary": ThreadSummary.from_wire,
    "ThreadSnapshot": ThreadSnapshot.from_wire,
    "TurnReceipt": TurnReceipt.from_wire,
    "TurnResult": TurnResult.from_wire,
    "TurnEvent": TurnEvent.from_wire,
    "FilesystemApprovalTarget": FilesystemApprovalTarget.from_wire,
    "FilesystemApprovalRequest": FilesystemApprovalRequest.from_wire,
    "McpStartupApprovalRequest": McpStartupApprovalRequest.from_wire,
}


def _wire_key(name: str) -> str:
    head, *tail = name.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in tail)


def _normalize_wire(value: object) -> object:
    if isinstance(value, TurnEvent):
        return _normalize_wire(value.data)
    if isinstance(value, McpStdioStartupTarget):
        return {
            "kind": "stdio",
            **_normalize_dataclass(value),
        }
    if isinstance(value, McpHttpStartupTarget):
        return {
            "kind": "http",
            **_normalize_dataclass(value),
        }
    if is_dataclass(value) and not isinstance(value, type):
        return _normalize_dataclass(value)
    if isinstance(value, tuple | list):
        return [_normalize_wire(item) for item in value]
    if isinstance(value, dict):
        return {key: _normalize_wire(item) for key, item in value.items()}
    return value


def _normalize_dataclass(value: object) -> dict[str, Any]:
    return {
        _wire_key(field.name): _normalize_wire(getattr(value, field.name))
        for field in fields(value)
    }


class PublicWireConformanceTests(unittest.TestCase):
    def test_corpus_tracks_every_exported_handwritten_decoder(self) -> None:
        exported = {
            name
            for name in psychevo.__all__
            if callable(getattr(getattr(psychevo, name), "from_wire", None))
        }
        self.assertEqual(set(_CORPUS["decoders"]), exported)
        self.assertEqual(set(_DECODERS), exported)

    def test_public_decoders_accept_and_normalize_the_shared_valid_corpus(
        self,
    ) -> None:
        self.assertEqual(_CORPUS["schemaVersion"], 1)
        for name, cases in _CORPUS["decoders"].items():
            decoder = _DECODERS[name]
            for fixture in cases["valid"]:
                with self.subTest(decoder=name, fixture=fixture["name"]):
                    decoded = decoder(fixture["value"])
                    self.assertEqual(_normalize_wire(decoded), fixture["value"])

    def test_public_decoders_reject_the_shared_invalid_corpus(self) -> None:
        for name, cases in _CORPUS["decoders"].items():
            decoder = _DECODERS[name]
            for fixture in cases["invalid"]:
                with self.subTest(decoder=name, fixture=fixture["name"]):
                    with self.assertRaises((KeyError, TypeError, ValueError)):
                        decoder(fixture["value"])


if __name__ == "__main__":
    unittest.main()
