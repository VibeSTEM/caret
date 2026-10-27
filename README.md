# Caret

<p align="center">
  <img alt="Written in Rust and Qt 6" src="https://img.shields.io/badge/written%20in-Rust%20%2B%20Qt%206-2b47b5?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on macOS, Windows and Linux" src="https://img.shields.io/badge/runs%20on-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-3b5bdb?style=flat-square">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-2b47b5?style=flat-square"></a>
</p>

Caret 是轻量的所见即所得结构化编辑器，计划用 Rust 实现领域规则，用 Qt 6 / QML 实现桌面界面。产品目标和里程碑见[需求与范围](ai-docs/architecture/requirements.md)。

仓库已建立 Cargo、CMake 和 QML 工程。`cargo run` 打开 M0 文档窗口，页面仍是占位，正文编辑和保存尚未实现。CI、夜间检查、桌面打包与 GoogleTest SDK 工作流已编写，远程验收状态见[任务 006](ai-docs/task/006-infra-ci.md)。

在仓库根目录使用 `cargo format --check`、`cargo lint --check`、`cargo audit` 和 `cargo test --workspace --locked`；`cargo quality` 聚合这些检查。首次运行需要 git、rustup 和[平台前置](ai-docs/standards/dependency-acquisition.md#宿主前置)，并联网取得锁定工具。裸 `cargo test` 只测 launcher。桌面安装使用 `cargo caret-package`，SDK 的独立生产和消费见[CI 规范](ai-docs/standards/ci.md)。

| 内容 | 入口 |
| --- | --- |
| 协作规则 | [仓库协作指南](AGENTS.md) |
| 工作队列与实现状态 | [任务索引](ai-docs/task-index.md) |
| 产品行为与架构 | [架构文档](ai-docs/architecture/README.md) |
| 工程栈、版本和长期规则 | [技术规范](ai-docs/standards/README.md) |
| 外部项目调查 | [调查索引](ai-docs/research/README.md) |
