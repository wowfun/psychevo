from __future__ import annotations

import shutil
import subprocess
from pathlib import Path


class PathPickerUnavailable(RuntimeError):
    pass


def pick_file_paths(*, multiple: bool = True) -> list[str]:
    errors: list[str] = []
    try:
        return tkinter_file_paths(multiple=multiple)
    except Exception as exc:  # noqa: BLE001 - optional local GUI backend.
        errors.append(f"tkinter: {exc}")

    result = command_file_paths(multiple=multiple, errors=errors)
    if result is not None:
        return result

    detail = "; ".join(errors) if errors else "no supported native file picker found"
    raise PathPickerUnavailable(f"native file picker unavailable: {detail}")


def tkinter_file_paths(*, multiple: bool) -> list[str]:
    import tkinter as tk
    from tkinter import filedialog

    root = tk.Tk()
    root.withdraw()
    try:
        if multiple:
            selected = filedialog.askopenfilenames(parent=root)
            return normalized_paths(selected)
        selected = filedialog.askopenfilename(parent=root)
        return normalized_paths([selected] if selected else [])
    finally:
        root.destroy()


def command_file_paths(
    *,
    multiple: bool,
    errors: list[str],
) -> list[str] | None:
    for name, command in picker_commands(multiple=multiple):
        executable = shutil.which(name)
        if not executable:
            errors.append(f"{name}: not found")
            continue
        try:
            result = subprocess.run(
                [executable, *command],
                check=False,
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            errors.append(f"{name}: {exc}")
            continue
        if result.returncode == 0:
            return normalized_paths(result.stdout.splitlines())
        stderr = result.stderr.strip()
        if not stderr:
            return []
        errors.append(f"{name}: {stderr}")
    return None


def picker_commands(*, multiple: bool) -> list[tuple[str, list[str]]]:
    if multiple:
        return [
            ("zenity", ["--file-selection", "--multiple", "--separator=\n"]),
            ("yad", ["--file-selection", "--multiple", "--separator=\n"]),
            ("kdialog", ["--multiple", "--separate-output", "--getopenfilename"]),
        ]
    return [
        ("zenity", ["--file-selection"]),
        ("yad", ["--file-selection"]),
        ("kdialog", ["--getopenfilename"]),
    ]


def normalized_paths(values: object) -> list[str]:
    paths: list[str] = []
    for value in values or []:
        text = str(value or "").strip()
        if not text:
            continue
        path = Path(text).expanduser()
        paths.append(str(path if path.is_absolute() else path.resolve()))
    return paths
