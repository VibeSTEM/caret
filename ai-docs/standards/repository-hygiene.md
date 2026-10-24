# 仓库文件、产物与忽略规则

更新日期：2026-10-24。

## 归属

| 类别 | 约定 |
| --- | --- |
| 源码、QML、资源和 schema | 提交。生成输入必须能追溯 |
| `Cargo.lock`、工具链、共享 CMake presets | 提交 |
| `pyproject.toml` 和 `uv.lock` | 提交并保持一致 |
| 本机 CMake user presets | 不提交。示例可以提交，但不能含个人路径 |
| CXX、moc、rcc、临时 TS 和 QM | 留在构建树 |
| 小型夹具 | `tests/fixtures/<module>/`，记录来源和预期 |
| 日志、截图、覆盖率、包 | `artifacts/`、`coverage/` 或构建目录 |

根 `.gitattributes` 使用 `* text=auto eol=lf`。需要字节级比较的夹具不在断言前改写换行。

Cargo 和 native 产物都在根 `target/` 下。build script 遵守 `OUT_DIR`。忽略规则覆盖构建目录，不用 `*.qml`、`*.json` 或 `*.svg` 这类会误伤源文件的通配。Qt 和 CXX 的生成文件随构建树排除。

`.vscode/` 可以提交可移植的推荐设置。个人偏好放本机 exclude。`.env.example` 可以提交，真实环境文件不提交。

添加忽略规则时列出必须忽略和必须保留的路径，并用 `git check-ignore -v --no-index` 核对。已经跟踪的产物不会因为新规则自动退出索引。

不建立 `old/` 或 `backup/` 目录。废弃实现按[代码生命周期](code-lifecycle.md)删除。
