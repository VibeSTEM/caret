# 仓库协作指南

Caret 是轻量的所见即所得结构化编辑器。领域规则在 Rust，界面在 Qt 6 / QML。工程栈见[技术规范索引](ai-docs/standards/README.md)。`cargo run` 打开 M0 文档窗口，页面仍是占位。当前可以跑 `cargo fmt`、`cargo clippy --locked --workspace --all-targets -- -D warnings` 和 `cargo test --workspace --locked`。不带 `--workspace` 的 `cargo test` 只测 launcher。跨语言入口是 `cargo format --check`、`cargo lint --check` 和 `cargo quality`，前置与首次工具供给见[测试规范](ai-docs/standards/testing.md)。

只阅读当前任务相关的文档。工程栈以规范为准。外部调查只放在 [research](ai-docs/research/)。

非平凡代码、架构或文档改动开始前，必须阅读并执行[验证与评审标准](ai-docs/standards/validation-and-review.md)：按分支实际涉及的评审项分别委派 Terra/medium 领域检查，完成修复后由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读 final review。纯格式或单行拼写修复可按该标准豁免。

## 入口

| 工作 | 文档 |
| --- | --- |
| 任务状态与新任务 | [任务索引](ai-docs/task-index.md)、[任务模板](ai-docs/task/_template.md) |
| 产品行为 | [架构总览](ai-docs/architecture/README.md) |
| 版本、移植范围和编码规则 | [技术规范索引](ai-docs/standards/README.md) |
| 分层 | [分层规范](ai-docs/standards/layering.md) |
| 依赖与 `.toolchain/` | [依赖获取](ai-docs/standards/dependency-acquisition.md) |
| 构建与测试 | [测试规范](ai-docs/standards/testing.md) |
| 验证、独立评审与委派 | [验证与评审](ai-docs/standards/validation-and-review.md) |
| 提交、分支与 GitHub | [提交规范](ai-docs/standards/commits.md) |
| 注释、文件和兼容例外 | [注释](ai-docs/standards/comments.md)、[仓库文件](ai-docs/standards/repository-hygiene.md)、[代码生命周期](ai-docs/standards/code-lifecycle.md) |
| 快捷键 | [快捷键](ai-docs/architecture/interface/keyboard-shortcuts.md) |
