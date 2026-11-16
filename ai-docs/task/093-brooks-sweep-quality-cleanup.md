# 093 — 全仓 Brooks Sweep 质量清理记录（已实施）

- 状态：done
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

按用户要求在专用分支 `brooks-sweep-2026-11-16` 上执行 Brooks Full Sweep：11 个只读审查域扫描全仓，归并问题后按 Safe / Extended-Safe / Residual 分级修复，并保留可复核的验证证据。本任务记录 sweep 的已实施改动与残余登记；残余项各自建 094–102，实现细节写在对应任务中。

## 必读

[验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)。

## 范围与非目标

范围内：R1–R6、T1–T6、技术债与架构四维度的安全修复；逐批 `cargo fmt` / `cargo clippy --workspace --all-targets -D warnings` / `cargo test --workspace --locked` 验证；独立只读复核。非目标：公共 API 破坏、跨模块结构重构、需要新增测试基础设施的修复（这些登记为 094–102，不在本任务应用）。

## 前置条件与待决策

无前置阻塞；`brooks-sweep-2026-11-16` 分支由 `55bd79b4` 创建，未 push。各提交按 `<type>(<scope>)` 拆分，消息写明 Task/Validation/Cleanup/Compat；因 sweep 先于本任务记录实施，提交中 Task 记为“无”，本任务为事后登记。

## 实施步骤

1. 11 域只读扫描 + 机械扫描（函数长度、文件规模、重复块、链接、测试选择器）。
2. 按模块分批修复；每批跑门禁并由未参与实现的 subagent 只读复核。
3. 复核发现回归时回滚并重做（layout 误删分支、doc 边界语义各一次）。
4. 残余归并为 094–102。

## 预计改动

40 个提交、72 文件、+6200/−4979：engine/fixture(Qt/C++)、caret-doc、caret-math、caret-layout、caret-app、caret-dsl-core、qml、tests/src、tests/cpp。

## 验收标准

- [x] 每批修复在完整工作树上通过 fmt/clippy/全量测试。
- [x] 独立只读复核覆盖各批 diff，发现的问题已修正。
- [x] 残余项已登记为带文件位置的后续任务（094–102）。
- [ ] 分支级独立 final review、PR 与 task 记录串接（待维护者授权 push/PR 后补）。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-17 | `cargo fmt --all -- --check` | 通过 | 通过 |
| 2026-11-17 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | 通过 | 通过 |
| 2026-11-17 | `cargo test --workspace --locked` | 36 suite 全绿 | exit 0，含 native CTest 11/11、qmllint、qmlformat |
| 2026-11-17 | ai-docs 相对链接扫描 | 0 broken | 0 broken |

## 风险与回退

分支未 push，回退即删除分支或按提交 revert。残余项涉及撤销/IME 语义或结构拆分时，按对应任务先补特征化测试再改。

## 决策与工作记录

- 2026-11-17：用户要求全部修复并允许拆分 commit；分 6 批实施，40 个提交。
- 2026-11-17：独立复核发现两处回归（layout 误删 Equation 模板分支；doc raw/解码器边界被统一导致 open() 结果变化），均已回滚/改为三策略共享内核后复验。
- 2026-11-17：残余 41 个证据点归并为 094–102。

## 完成摘要

已实施：40 个逻辑提交（72 文件）覆盖约 40 个证据点（约 24 个根因），四道门禁与独立复核均有记录。未覆盖：分支级 final review、PR 描述与 push（需维护者授权），以及 094–102 的残余结构/测试基础设施工作。
