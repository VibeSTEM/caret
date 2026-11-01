# 仓库协作指南

Caret 是轻量的所见即所得结构化编辑器。领域规则在 Rust，界面在 Qt 6 / QML。工程栈见[技术规范索引](ai-docs/standards/README.md)。`cargo run` 打开桌面文档窗口；基础文本编辑与 `.cdoc` 打开/保存已接入，后续功能状态见[任务索引](ai-docs/task-index.md)。当前可以跑 `cargo fmt`、`cargo clippy --locked --workspace --all-targets -- -D warnings` 和 `cargo test --workspace --locked`。不带 `--workspace` 的 `cargo test` 只测 launcher。跨语言入口是 `cargo format --check`、`cargo lint --check` 和 `cargo quality`，前置与首次工具供给见[测试规范](ai-docs/standards/testing.md)。

只阅读当前任务相关的文档。工程栈以规范为准。外部调查只放在 [research](ai-docs/research/)。

使用 `gh` 操作 GitHub 前先运行 `gh auth status`。宿主机已有登录凭据但沙箱无法访问时，通过权限提升在沙箱外运行 `gh`，不要在隔离沙箱中重复登录或刷新凭据；先说明所需操作并请求对应的沙箱权限。只有宿主机 `gh auth status` 也确认未登录或 scope 不足时，才引导维护者完成 GitHub 登录/授权，然后重新检查状态。`git push` 成功不代表 `gh` 已授权。创建 PR 只提交评审并触发 CI，不等同于合并；合并前确认 CI 结果和本次明确授权。

非平凡代码、架构或文档改动开始前，必须阅读并执行[验证与评审标准](ai-docs/standards/validation-and-review.md)：按分支实际涉及的评审项分别委派 Terra/medium 领域检查，完成修复后由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读 final review。纯格式或单行拼写修复可按该标准豁免。

新增或重构第一方 Rust、QML、C++ 核心源码时，必须按[源码署名规范](ai-docs/standards/source-attribution.md)的文件头样例添加 SPDX 许可证标识和项目署名；始终照仓库中经维护者确认的源码样例使用统一署名值。若尚无已确认的源码样例，先向维护者确认并建立样例。不得猜测或原样使用文档占位符，也不得给生成文件或第三方源码添加署名。批量补齐既有文件署名时单独提交，不与功能改动混合。

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
| 源码署名 | [署名规范](ai-docs/standards/source-attribution.md) |
| 快捷键 | [快捷键](ai-docs/architecture/interface/keyboard-shortcuts.md) |
