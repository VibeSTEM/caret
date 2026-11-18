#!/usr/bin/env python3
"""从 Rust 命令注册表生成 lupdate 可识别的候选标签源。"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REGISTRY = ROOT / "crates/caret-engine/src/editing/command_registry.rs"
QML = ROOT / "qml/Canvas/CommandSuggestionPopup.qml"


def labels_from_registry() -> list[str]:
    source = REGISTRY.read_text(encoding="utf-8")
    registered = re.findall(r'\blabel:\s*"([^"]+)"', source)
    math_candidates = re.findall(r'^\s*\("[^"\n]+",\s*"([^"]+)",\s*"[^"\n]*"\),?$', source, re.MULTILINE)
    return list(dict.fromkeys([*registered, *math_candidates]))


def replacement(labels: list[str]) -> str:
    entries = ", ".join(
        f'qsTranslate("CommandCompletion", "{label}")' for label in labels
    )
    return (
        "    // 此函数仅供 lupdate 扫描，运行时不会执行。\n"
        "    function translationSourcesForLupdateOnly() {\n"
        f"        return [{entries}];\n"
        "    }\n\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="只检查生成内容是否同步")
    args = parser.parse_args()
    current = QML.read_text(encoding="utf-8")
    marker = "    // 此函数仅供 lupdate 扫描，运行时不会执行。"
    if marker not in current:
        marker = "    // 此列表由 tools/i18n/generate_command_completion_sources.py 生成。"
    start = current.index(marker)
    end = current.index("    function labelFor(candidate) {", start)
    expected = replacement(labels_from_registry())
    if args.check:
        if current[start:end] != expected:
            print("CommandSuggestionPopup.qml 的候选翻译源未与 Rust 注册表同步")
            return 1
        return 0
    QML.write_text(current[:start] + expected + current[end:], encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
