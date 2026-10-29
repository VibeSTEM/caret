# 仓库文件、产物与忽略规则

更新日期：2026-10-28。

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
| 下载的工具、Qt、SDK、uv / Python | 仓库根 `.toolchain/`，不提交 |

根 `.gitattributes` 使用 `* text=auto eol=lf`。需要字节级比较的夹具不在断言前改写换行。

编译产物在根 `target/` 下：Cargo 输出、`target/native/<profile>` 和 build script 的 `OUT_DIR`。下载的依赖在 `.toolchain/`，两套忽略规则分开写。清理构建目录不会卸掉工具链。不用 `*.qml`、`*.json` 或 `*.svg` 这类会误伤源文件的通配。Qt 和 CXX 的生成文件随构建树排除。

`.vscode/` 可以提交可移植的推荐设置。个人偏好放本机 exclude。`.env.example` 可以提交，真实环境文件不提交。

添加忽略规则时列出必须忽略和必须保留的路径，并用 `git check-ignore -v --no-index` 核对。已经跟踪的产物不会因为新规则自动退出索引。

不建立 `old/` 或 `backup/` 目录。废弃实现按[代码生命周期](code-lifecycle.md)删除。

## 模块文件结构

Rust 模块需要拆分职责或单测明显增大时，使用 `module.rs` 作为模块入口，并在同级 `module/` 中放功能子模块和 `tests.rs`。较大的私有单测通过入口中的 `#[cfg(test)] mod tests;` 注册，保留父模块私有访问与测试归属；crate 根 `lib.rs` / `main.rs` 对应的测试放在 `src/tests.rs`。新建或重构模块时不采用 `module/mod.rs`；[Rust Reference](https://doc.rust-lang.org/reference/items/modules.html#module-source-filenames) 与 [The Rust Programming Language](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) 仍将其列为受支持的旧路径，并鼓励使用 `module.rs`，因为它更一致且避免编辑器中出现许多同名 `mod.rs`。不使用平铺的 `module_tests.rs`，也不只为行数阈值机械拆分；拆分应对应独立职责，并在评审中检查调用边界与重复实现。已有 `mod.rs` 在未涉及该模块重构时不要求仅为统一风格搬迁。

由多个可独立推导的构件组成一个领域投影时，可在功能模块下放 `atoms/`，每个 atom 只持有源节点的稳定 ID、范围或轻量引用；用一个明确的 composer/index 入口按文档顺序完成装配。atom 不能复制领域源数据、各自维护计数或重复承担最终组合策略；只有存在多个真实消费者或稳定公共契约时才导出 atom API。单一职责的小功能直接保留在模块内，不为了形式创建空 atoms 目录。
