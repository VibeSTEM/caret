# 102 — Brooks sweep 后续收敛项

- 状态：in-progress
- 依赖：093、095
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-18

## 目标与背景

Brooks sweep 的独立复核在已修复项之外给出若干"同族未收敛"点，均属低风险但值得一次收口：
- `commit_document_edit` 成为正文/结构/IME/格式记账入口后，仍有 5 处手写事务：块公式删除（`math_edit.rs` 约 4604）、数学树记账（约 5404）、数学文本待输入格式（`text_inline_environment.rs` 约 222/261）、文本 run 属性（`text_run_property.rs` 约 159）；
- `MathEdit` 构造入口收敛后，`inline_content.rs` 的构造点已在本分支开始前收敛；本任务剩余 `MathEdit::command_snapshot` 及测试的直接 struct 构造；
- raw codec 的节点树判定只返回 bool，把“未来未知节点格式（应保留为 Opaque 只读预览）”与“当前 schema 非法结构（应报格式错误）”折叠。`math_node_attributes_supported` 接收封闭的 `MathNodeKind`，本身无法识别未来 kind，因此三态结果放在 raw JSON 解码边界。

完成后：事务入口覆盖全部文档提交点，MathEdit 只有构造入口（含测试），文档校验用显式结果类型区分“未知/非法”。

## 必读

[验证与评审](../standards/validation-and-review.md)、[代码生命周期](../standards/code-lifecycle.md)、[分层](../standards/layering/rust.md)、任务 085（严格字段与消费者校验）、093。

## 范围与非目标

范围内：上述同族收敛；raw 数学节点树三态分类（含调用点与测试），并在未来节点导致整棵载体转为 Opaque 前检查已知兄弟的局部字段、结构和可见 ID；修复[Actions 44310447187](https://github.com/VibeSTEM/caret/actions/runs/44310447187)暴露的 apt 源 HTTP 429 短暂失败。非目标：把所有 typed 文档验证迁入 raw classifier；格式接受范围（除 085 已定义者）；无证据的性能改写。

## 前置条件与待决策

三态分类需与 085 的严格校验方案对齐；typed 文档模型仍负责全局不变量，raw classifier 只检查会因 Opaque 回退而绕过的可见结构。事务收敛需保证 PendingStyle 不分配新 ID、不推进代际的既有语义。

## 实施步骤

1. 逐个手写事务点接入 `commit_document_edit`（或说明为何口径无法表达，补 `DocumentEditSnapshot` 变体）。
2. `inline_content.rs` 已在本分支开始前改用构造入口；本任务将 `command_snapshot` 和测试改用构造入口。
3. 文档校验改为枚举结果（`Known` / `FutureOpaque` / `Invalid`），调用点按语义分支；补行为测试钉住“未知→只读预览、非法→报错”。
4. 全量门禁 + 独立评审；PR CI 通过后完成。

## 预计改动

`crates/caret-engine/src/editing/session/math_edit.rs`、`math_edit/text_inline_environment.rs`、`math_edit/text_run_property.rs`、`session/inline_content.rs`、`crates/caret-doc/src/model/math.rs`、`storage_format/*`、相关 tests。

## 验收标准

- [x] 文档提交点全部走统一入口（或记录不可表达口径并加注释）。
- [x] `MathEdit` 直接 struct 构造点清零，测试也使用构造入口。
- [x] 未知/非法节点在 open 与预览路径上行为有测试且与 085 一致。
- [ ] 本地全量门禁与独立评审通过，PR CI 待通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo fmt --check` | 格式通过 | 通过（`cargo fmt` 后复核） |
| 2026-11-18 | `cargo test --locked -p caret-engine -p caret-doc` | 全绿 | 通过，964 项；doc tests 通过 |
| 2026-11-18 | `cargo clippy --locked -p caret-engine -p caret-doc --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-18 | `cargo test --workspace --locked` | 全 workspace 通过 | 通过；Rust、native、QML 与应用测试通过（含最新 codec 回归用例） |
| 2026-11-18 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-18 | `git diff HEAD --check`、`actionlint .github/workflows/ci.yml`、apt 脚本 `bash -n` | diff、workflow 和 shell 语法通过 | 全部通过 |

## 风险与回退

第 3 点会改 `open()` 的边界语义，必须与 085 一起设计并补差分测试（sweep 曾在同类改动上出现预览↔硬错误回归，已由策略参数化修复，可复用该模式）。

## 决策与工作记录

- 2026-11-17：复核提出上述同族点；本任务收口，避免“修了一半”的分叉继续扩散。
- 2026-11-18：事务提交统一通过 `commit_document_edit`；显式历史快照由入口统一分配 state id，并以补丁决定是否推进文档代际。`MathEdit::command_snapshot` 改用既有构造入口并保留源码投影身份。
- 2026-11-18：数学节点验证返回 `Known/FutureOpaque/Invalid`；当前 schema 中已知格式错误在预览处理前拒绝，未来未知内容仍完整保留为只读 Opaque。raw capability 与 retired-field 扫描继续使用各自原有容忍边界。
- 2026-11-18：新增/调整回归用例覆盖未来节点、已知非法属性/结构、缺失/非法 id、Matrix 槽位、嵌套未知操作数及混合未知与非法兄弟顺序；待输入样式测试确认不改变文档、历史 state id 与文档代际，后续输入只推进一次代际。
- 2026-11-18：`inline_content.rs` 的构造点在本分支开始前已收敛；分领域 Terra 检查已完成。最终独立评审发现的嵌套未来节点、槽位映射及 id 校验问题均已修复，最终完整 diff 独立评审通过。
- 2026-11-18：再补 TextFlow Run 与行内环境的已知字段校验，并在 future opaque 回退前拒绝同一可见公式树内重复 ID。全局文档不变量仍由 typed model validation 负责；未知 kind 的载荷不递归解释。
- 2026-11-18：修复中 CI 链接的失败不是代码检查失败：`cargo lint (machete, ubuntu)` 在 Linux apt 前置步骤被 `packages.microsoft.com` 的 HTTP 429 限流阻断。apt 更新使用 90 秒单次上限，包下载使用 180 秒单次上限，均最多三次并采用 30/60 秒指数退避及 0–15 秒抖动；changes、dependency-audit 与 format job 超时统一为 30 分钟。Linux 实际安装与重试行为由 PR CI 验证。

## 完成摘要

本地 workspace 门禁与独立评审已通过；等待 PR CI 验证 Linux apt 重试和代码改动。
