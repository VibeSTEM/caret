# 097 — 跨 crate 数学符号与属性单一来源

- 状态：done
- 依赖：093、084（行内与数学内容形状）
- 优先级：P1
- 创建 / 更新：2026-11-17 / 2026-11-18

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

`caret-doc` 是 `.cdoc` 数学属性格式的所有者；跨 crate 共享需公开稳定的值域/描述符，并在架构文档中明确其格式契约。保留当前源码语义：属性值 `ring` 规范导出为 `\mathring`，输入 `\ring` 继续拒绝；不扩展 LaTeX 子集。

## 实施步骤

1. 列出全部重复表与差异点，逐项给出"值域是否应一致、差异是否刻意"的结论。
2. 在 `caret-doc` 提供唯一常量/函数（如 `math_accent_attribute_values()`），caret-math 引用；保留 caret-math 对外常量（可改为重导出）。
3. 为每组表加"两层一致"测试（含新增值时的失败提示）。
4. 全量门禁 + 独立评审。

## 预计改动

`crates/caret-doc/src/model/math.rs`、`crates/caret-math/src/symbols.rs`、`parser.rs`、`serialize.rs`、`layout/convert.rs`；可能新增 `caret-doc` 的 pub(crate) 常量模块。

## 验收标准

- [x] accent/matrix/em-mu/属性值域各组只有一处定义。
- [x] 新增一个 accent/matrix 环境时只需改一处、且有测试覆盖双向一致。
- [x] 全量门禁与独立评审通过；格式契约文档同步。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo fmt --all` | Rust 格式通过 | 通过 |
| 2026-11-18 | `cargo test --locked -p caret-doc -p caret-math` | 文档契约与数学解析/布局测试全绿 | `caret-doc` 83 项、`caret-math` 146 项通过 |
| 2026-11-18 | `cargo quality` | 完整仓库质量门禁通过 | 通过；cargo-deny 报告 deny.toml 中 5 个未遇到的许可白名单项警告，无 deny 错误 |
| 2026-11-18 | `git diff --check` | 无空白错误 | 通过 |

## 风险与回退

可能在"统一值域"时改变对畸形文档的接受范围；只做单一来源，不改变既有一致部分的行为，差异项先记录再定。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已确认 accent 漂移（30 vs 29）与 matrix/em-mu 重复，未在 sweep 内改（跨 crate 格式契约）。
- 2026-11-18：以 `caret-doc` 作为格式权威，集中维护数学原子类别、accent 语义、定界符值域/尺寸和矩阵环境描述符；`caret-math/src/dimension.rs` 统一 em/mu 有理长度解析与格式化。`caret-math` 通过共享描述符解析、序列化和布局；保留 `ring` 仅由 `\mathring` 输入的既有差异。
- 2026-11-18：最终验证检查所有 10 种持久矩阵环境的解析样式、列规格、规范环境导出和回读；30 种 accent 描述符双向校验。旧公开 `ACCENTS` 列表从共享表派生，过滤 `ring`/`mathring` 特例。
- 2026-11-18：布局 API 对手工构造的 `Matrix.style=gathered` 保留旧有继承单元格字号行为；`gathered` 仍不是持久格式或 TeX 解析支持值。此兼容路径只服务现有公开布局调用。
- 2026-11-18：Terra 领域检查覆盖文档契约、矩阵行为和 em/mu 有理长度；最终 Sol 只读评审检查完整 diff。发现项均已修复并复核。尺寸解析上限和序列化小数截断保持旧行为；部分序列化结果因 18 位总长限制不能回读，已记录在架构文档。

## 完成摘要

数学语义与属性值域现由 `caret-doc` 单点定义，`caret-math` 共享消费；矩阵/accent 双向一致测试和文档契约已同步。完整质量门禁及独立评审通过，task 状态为 `done`。
