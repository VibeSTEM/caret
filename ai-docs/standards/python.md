# Python 工具环境

更新日期：2026-10-25。工具版本见[技术基线](baseline.md)。uv 管理 Python 质量工具；解释器和 wheel 缓存在 `.toolchain/tools/python/<id>/`，见[依赖获取](dependency-acquisition.md)。Python 不是桌面运行时。

## 项目规则

- `pyproject.toml` 声明 `requires-python = ">=3.12,<3.15"`。提交 `uv.lock`。不把依赖装进系统 Python，不提交 `.venv`。
- 当前工具是 `cmakelang` 和 `cppcheck` 的预编译 wheel。增加工具时分开 dependency group 并更新锁。
- CMake 格式经 `cargo format`，规则经 `cargo lint cmake`。工具失败必须返回非零。Cargo 用 `uv run --locked` 调用，不假设机器上有 Ruff、Black 或 pytest。
- 路径用 `pathlib`。外部进程用参数列表，写明 cwd、环境和退出码。
- import 模块时不启动界面、不改文档、不做耗时计算。

Cppcheck 是开发期工具，按它自己的许可证随工具链保留，不链接进应用。
