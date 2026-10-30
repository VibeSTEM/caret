#!/usr/bin/env python3
"""在 main 分支提交或推送前检查 Rust 格式。"""

from runtime import check_main_format, relaunch_with_managed_python

relaunch_with_managed_python()
check_main_format()
