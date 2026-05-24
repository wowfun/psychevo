import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

port_file = Path(sys.argv[1])
request_log = Path(sys.argv[2])


class Handler(BaseHTTPRequestHandler):
    request_count = 0

    def log_message(self, fmt, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length).decode("utf-8", errors="replace")
        Handler.request_count += 1
        with request_log.open("a", encoding="utf-8") as out:
            out.write(
                json.dumps(
                    {"index": Handler.request_count, "path": self.path, "body": body}
                )
                + "\n"
            )

        if self.path.rstrip("/") != "/v1/chat/completions":
            self.send_response(404)
            self.end_headers()
            return

        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.end_headers()

        if "Title this user request" in body or "Generate a concise title" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_title",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {"content": "Inspect Fixture Ledger"},
                            "finish_reason": "stop",
                        }
                    ],
                },
                delay=0.1,
            )
        elif "call_agent_translate_vhs" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_agent_parent_final",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "Translation complete: 添加了带有运行中和可用标签页的全屏 /agents 控制台。"
                            },
                            "finish_reason": "stop",
                        }
                    ],
                },
                delay=0.2,
            )
        elif "Translate the VHS sentence to Chinese" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_agent_child",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "reasoning_content": "Inspecting the translation request inside the child session."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=3.0,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_agent_child",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "reasoning_content": " Checking terminology while the child session is open."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=4.0,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_agent_child",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "添加了带有运行中和可用标签页的全屏 /agents 控制台。"
                            },
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 12000,
                        "completion_tokens": 2500,
                        "total_tokens": 14500,
                    },
                }
            )
        elif "Subagent foreground VHS fixture" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_agent_parent_tool",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_agent_translate_vhs",
                                        "function": {
                                            "name": "Agent",
                                            "arguments": "{\"agent_type\":\"translate\",\"prompt\":\"Translate the VHS sentence to Chinese: Added the fullscreen /agents console with Running and Available tabs.\",\"task_name\":\"Translate user message to Chinese\"}",
                                        },
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                },
                delay=0.2,
            )
        elif "Interrupted exec command fixture" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_interrupt",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "Starting an exec command that should be interrupted."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=0.2,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_interrupt",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_interrupted_exec",
                                        "function": {
                                            "name": "exec_command",
                                            "arguments": "{\"cmd\":\"sleep 60\"}",
                                        },
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                }
            )
        elif "visible-write-output.md" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_visible_write_final",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {"content": "VISIBLE_WRITE_FINAL"},
                            "finish_reason": "stop",
                        }
                    ],
                },
                delay=0.2,
            )
        elif "Visible write preamble fixture" in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_visible_write",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "Now I have all the data needed. Let me write the complete report."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=1.0,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_visible_write",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_visible_write",
                                        "function": {
                                            "name": "write",
                                            "arguments": "{\"path\":\"visible-write-output.md\",\"content\":\"VISIBLE_WRITE_FINAL\"}",
                                        },
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                }
            )
        elif "Reasoning-only table bottom scroll fixture" in body:
            rows = "\n".join(
                f"| {index} | **Thinking Story {index}** - reasoning-only Markdown table row used to validate bottom scrolling | {200 + index} |"
                for index in range(1, 22)
            )
            content = (
                "Reasoning-only final report\n\n"
                "| # | Topic | Score |\n"
                "|---|---|---|\n"
                f"{rows}\n\n"
                "REASONING_ONLY_BOTTOM_MARKER: metadata below must remain reachable."
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_reasoning_only",
                    "model": "mock-model",
                    "choices": [],
                    "usage": {
                        "prompt_tokens": 260,
                        "completion_tokens": 160,
                        "total_tokens": 420,
                    },
                },
                delay=0.2,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_reasoning_only",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {"reasoning_content": content},
                            "finish_reason": "stop",
                        }
                    ],
                }
            )
        elif "Clarify VHS fixture" in body and "call_clarify_vhs" not in body:
            self.send_event(
                {
                    "id": "resp_tui_capture_clarify",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_clarify_vhs",
                                        "function": {
                                            "name": "clarify",
                                            "arguments": json.dumps(
                                                {
                                                    "questions": [
                                                        {
                                                            "question": "Which Reddit API path should pevo use?",
                                                            "options": [
                                                                {
                                                                    "label": "Register app (Recommended)",
                                                                    "description": "Create a Reddit app and use OAuth credentials",
                                                                },
                                                                {
                                                                    "label": "Public JSON endpoint",
                                                                    "description": "Use unauthenticated JSON with tighter limits",
                                                                },
                                                            ],
                                                        }
                                                    ]
                                                }
                                            ),
                                        },
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                },
                delay=0.2,
            )
        elif "Long markdown bottom scroll fixture" in body:
            rows = "\n".join(
                f"| {index} | **Story {index}** - long Markdown table row used to validate transcript bottom scrolling | {100 + index} |"
                for index in range(1, 34)
            )
            content = (
                "# Long Markdown Scroll Fixture\n\n"
                "| # | Topic | Score |\n"
                "|---|---|---|\n"
                f"{rows}\n\n"
                "LONG_MARKDOWN_BOTTOM_MARKER: metadata below must remain reachable."
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_long",
                    "model": "mock-model",
                    "choices": [],
                    "usage": {
                        "prompt_tokens": 240,
                        "completion_tokens": 180,
                        "total_tokens": 420,
                    },
                },
                delay=0.2,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_long",
                    "model": "mock-model",
                    "choices": [
                        {"delta": {"content": content}, "finish_reason": "stop"}
                    ],
                }
            )
        elif Handler.request_count == 1:
            self.send_event(
                {
                    "id": "resp_tui_capture_1",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "I'll inspect fixture.txt before summarizing the ledger."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=0.2,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_1",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "reasoning_content": "Inspecting fixture.txt and the TUI ledger..."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=0.4,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_1",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "reasoning_content": "Preparing an exec_command tool call."
                            },
                            "finish_reason": None,
                        }
                    ],
                },
                delay=0.4,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_1",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_exec_fixture",
                                        "function": {
                                            "name": "exec_command",
                                            "arguments": "{\"cmd\":\"sleep 2 && cat fixture.txt\"}",
                                        },
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                }
            )
        else:
            self.send_event(
                {
                    "id": "resp_tui_capture_2",
                    "model": "mock-model",
                    "choices": [],
                    "usage": {
                        "prompt_tokens": 120,
                        "completion_tokens": 38,
                        "total_tokens": 158,
                    },
                },
                delay=0.2,
            )
            self.send_event(
                {
                    "id": "resp_tui_capture_2",
                    "model": "mock-model",
                    "choices": [
                        {
                            "delta": {
                                "content": "SNAPSHOT_DEMO_FINAL: fixture.txt was read and the evidence ledger is ready."
                            },
                            "finish_reason": "stop",
                        }
                    ],
                }
            )
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def send_event(self, value, delay=0.0):
        self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode("utf-8"))
        self.wfile.flush()
        if delay:
            time.sleep(delay)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port_file.write_text(str(server.server_address[1]), encoding="utf-8")
server.serve_forever()
