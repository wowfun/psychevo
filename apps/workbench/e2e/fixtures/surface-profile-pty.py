#!/usr/bin/env python3
"""Drive the real fullscreen TUI without retaining terminal content."""

from __future__ import annotations

import argparse
import errno
import fcntl
import json
import os
import selectors
import signal
import struct
import sys
import termios


def emit(value: dict[str, object]) -> None:
    sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def resize(master_fd: int, columns: int = 120, rows: int = 40) -> None:
    packed = struct.pack("HHHH", rows, columns, 0, 0)
    fcntl.ioctl(master_fd, termios.TIOCSWINSZ, packed)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pevo", required=True)
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--model", required=True)
    args = parser.parse_args()

    child_pid, master_fd = os.forkpty()
    if child_pid == 0:
        argv = [
            args.pevo,
            "tui",
            "--new",
            "--model",
            args.model,
            "--cd",
            args.cwd,
        ]
        os.execvpe(args.pevo, argv, os.environ.copy())

    resize(master_fd)
    os.set_blocking(master_fd, False)
    selector = selectors.DefaultSelector()
    selector.register(master_fd, selectors.EVENT_READ, "terminal")
    selector.register(sys.stdin.buffer, selectors.EVENT_READ, "control")
    emit({"event": "started", "pid": child_pid})

    exit_code: int | None = None
    try:
        while exit_code is None:
            for key, _ in selector.select(timeout=0.1):
                if key.data == "terminal":
                    try:
                        # Terminal output may contain prompts and responses. It is deliberately
                        # drained and discarded instead of crossing the fixture boundary.
                        while os.read(master_fd, 65536):
                            pass
                    except BlockingIOError:
                        pass
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                else:
                    line = sys.stdin.buffer.readline()
                    if not line:
                        raise EOFError
                    command = json.loads(line.decode("utf-8"))
                    command_id = command.get("id")
                    if command.get("command") == "type":
                        text = command.get("text")
                        if not isinstance(text, str):
                            raise ValueError("type command requires text")
                        os.write(master_fd, text.encode("utf-8") + b"\r")
                        emit({"event": "written", "id": command_id})
                    elif command.get("command") == "quit":
                        os.write(master_fd, b"/quit\r")
                        emit({"event": "quit-written", "id": command_id})
                    elif command.get("command") == "resize":
                        resize(
                            master_fd,
                            int(command.get("columns", 120)),
                            int(command.get("rows", 40)),
                        )
                        emit({"event": "resized", "id": command_id})
                    else:
                        raise ValueError("unknown PTY control command")

            waited_pid, status = os.waitpid(child_pid, os.WNOHANG)
            if waited_pid == child_pid:
                exit_code = os.waitstatus_to_exitcode(status)
    except EOFError:
        os.kill(child_pid, signal.SIGHUP)
        _, status = os.waitpid(child_pid, 0)
        exit_code = os.waitstatus_to_exitcode(status)
    finally:
        selector.close()
        os.close(master_fd)

    emit({"event": "exited", "code": exit_code})
    return 0 if exit_code == 0 else exit_code


if __name__ == "__main__":
    raise SystemExit(main())
