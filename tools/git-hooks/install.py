#!/usr/bin/env python3
"""将当前仓库的 .githooks 配置为本地 Git hooks 目录。"""

import subprocess

from runtime import relaunch_with_managed_python, repository_root

relaunch_with_managed_python()
root = repository_root()
subprocess.run(
    ["git", "config", "--local", "core.hooksPath", ".githooks"],
    cwd=root,
    check=True,
)
print(f"Installed repository hooks at {root / '.githooks'}")
