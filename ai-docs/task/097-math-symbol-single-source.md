# 097 — 跨 crate 数学符号与属性单一来源

- 状态：planned
- 依赖：093、084（行内与数学内容形状）
- 优先级：P1
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

数学语义表在 `caret-doc`（格式所有者）与 `caret-math`（解析/排版）之间多份维护：
- accent 白名单：`caret-doc/src/model/math.rs` 30 值 vs `caret-math/src/symbols.rs` `ACCENTS` 29 值（`ring` 经 `\mathring` 特判），新增 accent 需改 5–6 处（ACCENTS、accent_attribute、accent_char、convert 特判、doc 白名单）；
- matrix 环境语义分散 4 处（`MATRIX_ENVIRONMENTS`、begin 分支、`parse_matrix_rows`、`convert.rs` 的 matrix）；
- `em/mu` 有理长度在 parser 与 serialize 各一套 `gcd/reduce/decimal`；
- 属性值域（accent/delimiter/atomClass 等）在文档校验与 caret-math 各有一份字面表。

完成后：这些"命令名 ↔ 属性值 ↔ 渲染"映射各只有一处定义（caret-doc 侧为格式所有者，caret-math 向下依赖），并有一致性测试防止漂移。

## 必读

[分层：Rust 分级](../standards/layering/rust.md)（caret-math 依赖 caret-doc 方向）、[架构：公式](../architecture/editor/math.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：把共享表移到低层 crate 的 `pub(crate)`/公开常量并由两层引用；补一致性测试。非目标：改 LaTeX 语法或渲染行为；不新增跨层反向依赖。

## 前置条件与待决策

确认 `caret-doc` 是否要公开具体值列表（影响格式契约文档）；`ring` 的对外语义（`\ring` 是否应被接受）需先定，再统一两张表。

## 实施步骤

1. 列出全部重复表与差异点，逐项给出"值域是否应一致、差异是否刻意"的结论。
2. 在 `caret-doc` 提供唯一常量/函数（如 `math_accent_attribute_values()`），caret-math 引用；保留 caret-math 对外常量（可改为重导出）。
3. 为每组表加"两层一致"测试（含新增值时的失败提示）。
4. 全量门禁 + 独立评审。

## 预计改动

`crates/caret-doc/src/model/math.rs`、`crates/caret-math/src/symbols.rs`、`parser.rs`、`serialize.rs`、`layout/convert.rs`；可能新增 `caret-doc` 的 pub(crate) 常量模块。

## 验收标准

- [ ] accent/matrix/em-mu/属性值域各组只有一处定义。
- [ ] 新增一个 accent/matrix 环境时只需改一处、且有测试覆盖双向一致。
- [ ] 全量门禁与独立评审通过；格式契约文档同步。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-doc -p caret-math` | 全绿 | 未执行 |

## 风险与回退

可能在"统一值域"时改变对畸形文档的接受范围；只做单一来源，不改变既有一致部分的行为，差异项先记录再定。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已确认 accent 漂移（30 vs 29）与 matrix/em-mu 重复，未在 sweep 内改（跨 crate 格式契约）。

## 完成摘要

未完成。
