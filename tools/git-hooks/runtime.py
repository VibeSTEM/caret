"""为仓库 Git hook 选择 .toolchain 中托管的 Python 解释器。"""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def managed_python() -> Path:
    root = repository_root() / ".toolchain" / "tools" / "python" / "venv"
    candidates = (
        root / "Scripts" / "python.exe",
        root / "bin" / "python.exe",
        root / "bin" / "python",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise SystemExit(
        "Caret 的托管 Python 尚未安装。请先运行一次 `cargo format` 供给 .toolchain Python。"
    )


def relaunch_with_managed_python() -> None:
    executable = managed_python()
    if Path(sys.executable).resolve() == executable.resolve():
        return
    os.execv(
        str(executable),
        [str(executable), str(Path(sys.argv[0]).resolve()), *sys.argv[1:]],
    )


def check_main_format() -> None:
    root = repository_root()
    print("Checking Rust formatting before a main branch operation...", flush=True)
    result = subprocess.run(["cargo", "format", "--check"], cwd=root, check=False)
    raise SystemExit(result.returncode)
