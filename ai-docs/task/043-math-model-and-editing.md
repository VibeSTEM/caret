# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

为可编辑公式建立版本化 Caret 数学树，并实现公式创建、内部编辑、位置和键盘导航。公式树属于 `.cdoc` 稳定文档模型；`latex-rust` AST 只能存在 `caret-math` 适配边界。043 创建低层 `caret-math` parser crate，并更新 Rust 分级与依赖图；044 在此基础上扩展布局 API。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令输入](../architecture/editor/command-input.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [存储格式](../architecture/editor/document-model/document-storage.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现 `InlineMath` 段内原子对象、块级 `Equation` 单行显示公式、块级 `Align` 多行对齐公式的 Caret 持久化类型、节点位置、占位符和公式内 `MathPos`。首版无自动编号、无 `\label` / `\ref`；支持的分隔符、行分隔和失败保留规则以 011 为准。保留 `defaultMathFontFamily`，新文档默认 STIX Two Math，首版只支持该字体。

本任务不实现 Qt 绘制和大范围布局优化，不写入第三方 AST，不实现表格行列编辑。

## 前置条件与待决策

010 结构导航和 042 后端验证需完成。`caret-math` 位于 Rust L1，不依赖其他 workspace crate；`caret-engine` 到 `caret-math` 的允许边已登记，`caret-doc` 的持久化 schema 不依赖它或第三方类型。043 在 parser/Token/AST 边界提供源码字节范围与预处理偏移映射，供失败导入定位；成功导入后以 Caret 树为权威。失败导入保留原公式源与可定位错误；成功解析/规范化后是否保留原始字节由 schema/导出契约决定，不假设源字符串与 Caret 树一一对应。布局盒到 Caret 数学节点的 provenance 由 044 实现，与 parser 源码范围分开。

## 实施步骤

1. 更新 Rust crate 分级和依赖图，在 L1 创建不依赖 workspace crate 的 `caret-math` parser 边界；为 parser/Token/AST 增加源码字节范围及预处理偏移映射。`caret-engine` 调用它解析后转换为 Caret 自有 schema，schema 不存第三方 AST。
2. 定义 schema 版本、`InlineMath`/`Equation`/`Align` 序列化表示、稳定 ID、源片段保存方式、MathPos 和 32 层树深度/输入长度上限；验证 Caret 树与 parser 递归预算的转换边界，比较树节点布局并测量内存占用。
3. 实现公式插入命令、公式内部 AST 编辑、占位符导航、左右上下移动、退出公式与错误恢复；LaTeX 命令由 `caret-math` parser 处理，不触发正文命令模式，`\\` 仅在 `Align` 中表示公式行分隔。
4. 接入原子事务与撤销/重做；解析或编辑失败不部分更改正文，未支持命令保留原输入并定位错误。
5. 覆盖新建、编辑、保存重开、旧/未知字段、异常深度、跨段选区边界和位置映射的行为测试。
6. 更新 011 与架构文档中的具体 schema/迁移决策，独立评审后复验。

## 预计改动

新增 `crates/caret-math`（L1 parser 边界）；调整分级/依赖图；`crates/caret-doc` 数学节点和验证/序列化，`crates/caret-engine` 公式命令与 MathPos/导航，`caret-app` 只在存储或命令边界确有需要时调整。044 扩展 `caret-math` 的 layout API。

## 验收标准

- [ ] 三种公式类型在 `.cdoc` 中为 Caret 自有 schema，往返后节点、源内容、ID 与位置语义稳定。
- [ ] 数学树避免每个符号单独分配字符串与堆对象；使用典型、深层及大量公式样本记录节点数、堆分配数和驻留内存后再确定紧凑布局。
- [ ] 自动编号、编号版 `equation`/`align`、label/ref 明确不支持且不会静默丢失源文本或编号语义。
- [ ] 键盘进入、内部移动、占位符跳转、离开和插入点落位符合 011。
- [ ] 失败原子；超深/过长输入不能导致进程崩溃；未知 `defaultMathFontFamily` 原样保留，实际回退到 STIX 时有诊断。
- [ ] 表格内公式可以由稳定块/行内结构表示，但与表格的可视集成留给 044。
- [ ] 命令级/模型行为测试、格式和 lint 通过；独立评审通过。

## 验证计划与结果

| 日期       | 环境 / 命令                            | 预期                                   | 实际结果                                             |
| ---------- | -------------------------------------- | -------------------------------------- | ---------------------------------------------------- |
| 2026-10-31 | `cargo test -p caret-math --locked --offline` | Parser 适配、后端无关语法树、深度/节点/长度边界、错误源保留 | 通过，16 项单元测试；尚未覆盖语法树到持久化 Caret schema 的转换 |
| 2026-10-31 | `cargo clippy --locked --offline -p caret-math --all-targets -- -D warnings`、`cargo fmt --all -- --check` | crate lint 与格式 | 通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | 数学 DTO、保存重开、正文原子偏移、Enter 与历史状态 | 通过：`caret-doc` 42 项、`caret-engine` 194 项 |
| 2026-10-31 | `cargo check --locked --offline --workspace` | 所有 crate 能识别新增持久化节点 | 通过；数学绘制和块级布局仍由 044 实现 |
| 2026-10-31 | `cargo lint --check` | Rust、依赖、分层、CMake/QML/native lint | 通过全部 9 个检查阶段 |
| 2026-10-31 | `CARGO_HOME=/private/tmp/caret-cargo-audit-home cargo-deny --offline check` | 新增依赖后的 advisories、版本、许可与来源审计 | 通过；使用本地 RustSec 数据库快照 `2295466`（2026-10-30），当前沙箱无法刷新在线数据库 |
| 2026-10-31 | `cargo lint --check` | crate lint、依赖和分层、CMake/QML/native lint | 通过（最终代码改动后复跑） |
| —         | caret-engine 公式编辑与位置行为测试     | MathPos、内部编辑和导航契约成立          | 未执行                                               |
| —         | 高节点数预算与合并后故障注入回滚测试    | 预算边界及失败后正文/选区/历史保持不变  | 未执行                                               |

## 风险与回退

公式 schema 一旦发布需通过能力标识和版本迁移保持兼容；先验证必要字段和未知能力策略，再稳定持久化形式。布局器的库 AST 变化不能要求 `.cdoc` 迁移。

## 决策与工作记录

- 2026-10-29：从 011 拆出文档模型、编辑和导航任务；以 020/011 的产品语义为准。
- 2026-10-31：新增 L1 `caret-math`，锁定 `latex-rust 3.0.1`，执行 1 MiB 源码限制、32 层 parser 预算，确保 `Align` 仅接收 `aligned` 树，并通过 AST 检查拒绝自动编号环境以及嵌套的 `\\label`、`\\ref`、`\\tag`、`\\nonumber` 和 `\\notag`。失败对象借用并保留原始输入，当前位置范围目前只能精确到整段公式。
- 2026-10-31：`latex-rust 3.0.1` 不暴露 token/AST 字节范围；细粒度源跨度及预处理映射仍需上游 adapter patch，本阶段没有自行实现第二套 tokenizer/parser。解析结果到 Caret 数学树的转换、MathPos 与公式编辑事务尚未实现。
- 2026-10-31：`caret-doc` 加入版本 1 的自有数学树与 `InlineMath`、`Equation`、`Align` DTO；math 节点范围、深度、源码长度和文档全局 ID 在保存入口统一校验，三种节点通过独立 `.cdoc` 能力标记声明。已补保存重开测试。`caret-layout` 暂以原子占位符表达行内公式，块级公式布局返回未支持；正式数学布局、MathPos、解析器到 Caret 树的转换及事务编辑待续。
- 2026-10-31：`caret-math` 增加独立模块中的临时 `MathSyntaxTree`，不暴露 `latex-rust` 类型，覆盖当前 parser AST 节点、矩阵行/单元格、文本样式和可选槽位；它仍需显式转换为 `caret-doc` schema 才能持久化。迭代式预算检查按转换后的层级计入矩阵行、intertext 和占位节点，限制 32 层及 65,536 节点；尺寸保留精确有理数，定界符编码保留字符与命名类型差别。尚未提供细粒度源跨度或转换到 `.cdoc` 树的实现。
- 2026-10-31：Terra/medium schema 与编辑边界只读复核发现并促成修复：未知 `sourceRange` 字段往返保留；Align 行进入文档节点预算；公式能力登记；未来 schema/节点 kind 转安全预览；U+FFFC 按 UTF-8 字节计长；公式边界可插软换行；跨段 Enter 失败恢复文档、选区、索引和缓存；数学扩展字段纳入历史大小估算。复验覆盖 schema 源码/深度限制、三种数学节点未来 schema、公式后的跨段 Enter、历史扩展字段估算。仍未验证高节点数预算裁剪及实际 MathPos/布局交互。

## 完成摘要

未完成。
