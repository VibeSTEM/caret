# 043 — M3：公式文档模型与编辑

- 状态：done
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-06

## 目标与边界

建立 Caret 自有的可编辑数学树，并实现公式创建、内部编辑、位置和键盘行为。持久化、数学节点和编辑模型以[数学公式架构](../architecture/editor/math.md)及[文档模型](../architecture/editor/document-model.md)为权威；[011](011-impl-math.md) 管理总体验收。解析器和排版后端不得成为 `.cdoc` 的第二份状态。

043 覆盖 `caret-doc`、`caret-math`、`caret-engine` 的模型、映射与编辑事务。布局、绘制和命中归 [044](044-math-layout-and-rendering.md)，表格归 [045](045-table-model-and-editing.md)/[046](046-table-layout-and-canvas.md)。TextFlow 字体与行内格式复用正文模型；默认字体角色归 [064](064-builtin-font-roles.md)。直接 TeX 源码输入与转换归 [065](065-tex-math-input-conversion.md)；044 负责 `\math`、`\equation`、`\align` 的命令接入。首版不支持自动编号、`\label`、`\ref` 或 IME 数学源码输入。

## 完成状态

- [x] `InlineMath`、`Equation`、`Align` 使用 Caret 结构树、稳定 ID 与 schema v3；`.cdoc` 不保存 TeX/sourceRange/sourceMap，旧双表示与退役字段在入口拒绝，不做迁移。
- [x] 数学 `TextFlow` 复用正文 Run、属性与字素边界；单流输入/删除、Bold/Italic、underline、根层 Math/Text 拆分、`Mod+Backspace` 降级及 Align 导航接入编辑事务。
- [x] MathEdit 投影绑定 session/document generation；历史快照共享 `Arc<MathSourceMap>`，恢复校验目标、类型、光标、路径与范围，失败保持文档和历史状态。根层拆分后的空锚点进入树并有 undo/redo 覆盖。
- [x] InlineMath 根层 TextFlow 提升为 ParagraphContent，保留顺序、空文本、Run 属性和单层 InlineEnvironment 身份；固定数学操作数槽位不提升。
- [x] 从持久化 DTO 删除冗余 `MathTree.source` 与 `Align.source`；历史投影从树生成规范源码。
- [x] Math TextFlow 的插入、删除、Run 属性和行内格式直接按树路径提交 MathTree 事务；这些文本编辑不创建或更新 TeX/source map，且仅受数学树预算限制。用例覆盖超过 TeX 源码上限的输入、undo/redo 和空 TextFlow。
- [x] 将文本岛与数学结构之间的导航迁移为 MathTree 位置；普通 TextFlow 焦点不得依赖临时 TeX 投影。嵌套 TextFlow/数学节点、Align 空 cell/空行和无效路径失败状态有树分支行为用例。
- [x] 从 TextFlow、InlineEnvironment、MathTree 与 Align 持久 DTO 移除源码范围/sourceMap 字段；投影位置仅属于 `caret-math` sidecar。
- [x] LaTeX 解析降级为 MathTree 时同步构造独立源码 sidecar；覆盖 Tree 与 Align cell，sidecar 结构和范围不再从 DTO 字段回读。
- [x] 完成显式 TeX 投影的规范化与映射身份保持，覆盖相邻 TextFlow 合并、Font 语义、转义后源码预算边界及不支持树形的原子拒绝。普通树编辑不受 TeX 投影字节预算限制；Mod+Backspace 仅在 InlineMath 降级时生成投影。
- [x] 跨 TextFlow 选择、数学 TextFlow 的 Rust 组字事务及 app 输入上下文已接入；覆盖 UTF-16 replacement、失败原子性、revision 门禁、历史和撤销。IME 预编辑光标、下划线与候选窗几何仍由 044 验收；044 布局接通前不宣称桌面端视觉链路完成。
- [x] MathFlow TextFlow 的 Bold/Italic 环境和 Font 字体族设置都走宿主事务；折叠字体复用 `PendingInlineStyle`，插入时继承，Font 属性保留在 MathTree。源码映射只按 TeX 可表达的 `rm` / `sf` / `tt` 样式类对齐，不改写文档中的具体字体。字体角色默认值见 [064](064-builtin-font-roles.md)。
- [x] `\textsf` / `\texttt` 解析为 TextFlow 的 `Font` 环境，并与 `\mathsf` / `\mathtt` 数学字母表节点区分；具体字体无法由 TeX 表达时，映射保留树属性，TeX 序列化遵循通用导出损失规则。
- 范围外：[065](065-tex-math-input-conversion.md) 负责源码草稿入口、失焦、关闭和真实 AutoSave/timer 体验。

## 验收

- [x] schema、未知字段、能力声明、旧双表示拒绝和失败原子性具备行为用例。
- [x] 数学单 TextFlow 编辑/格式、根层拆分与降级、Align 范围、历史恢复和 undo/redo 有测试。
- [x] `caret-doc` 能将 InlineMath/Equation/Align 解码为保留稳定 ID、Run 属性及未知字段的类型化安全预览；`math_carriers_decode_and_open_as_typed_safe_preview_preserving_ids_and_unknown_fields` 覆盖三种载体，退役 schema/source/范围字段有拒绝用例。
- [x] sidecar 从 DTO 迁出并在解析时独立构造。
- [x] 跨 TextFlow 选择和数学 TextFlow 的 Rust 组字事务覆盖成功、边界、失败原子性与历史行为；app 查询使用数学文本岛及 UTF-16 端点，Source/Tree 分支不启用 IME。
- [x] MathFlow 行内格式与 Font 环境适配经宿主事务验证；覆盖嵌套选择、重复切换、属性继承、undo/redo 和未知元数据失败原子性。当前没有独立的通用环境拆分/合并命令契约；`Font` 字体渲染由 044 按共享 TextFlow 契约验收，草稿用户路径由 065 验收。
- 按维护者要求，本轮暂不处理覆盖率，不把覆盖率门槛记作通过或失败；行为用例与 workspace/native 验证独立执行。
- [x] 完成最终完整 diff review。没有可复现的优化前后基线，不宣称性能提升。

## 验证与评审记录

| 检查点 | 结果 | 未覆盖 |
| --- | --- | --- |
| MathTree、解析 sidecar 与普通 TextFlow 事务 | 解析时并行构造 `caret-math` sidecar；普通插入、删除、格式、字素边界、undo/redo 直接更新树。源码范围只保留在会话 projection；`TextFlow`、MathTree 和 Align DTO 不再含源码位置字段。`MathTextFlowPath` 只保存结构路径和 Align cell。跨 TextFlow 选区及 Rust/app 数学文本组字事务已接通。 | 源码 projection 仍用于显式 Source 分支；结构位置迁移的剩余边界需通过完整审查确认。IME 下划线、光标和候选窗几何由 044 接入。 |
| 数学 TextFlow IME 与会话状态 | app 仅向 Text 分支提供 surrounding text 与 UTF-16 端点；预编辑使用事务克隆预览，提交按 MathTree 单事务记录。Source/Tree 不启用 IME；无效替换、过期 revision、预编辑期间直接改选区均拒绝且保留状态。有效数学编辑标记文档 dirty，但不会制造源码草稿。 | Qt/QML 的数学预编辑几何尚未接入；044 页面布局前不验证视觉反馈。`.cdoc` 数学可写保存、重开及 `DocumentSession` 往返由 044 验收。源码草稿生命周期属于 065。 |
| 2026-11-05 DTO 清理与结构回归 | 全仓清除旧 `TextFlow.sourceRange/sourceMap`、`InlineEnvironment.sourceRange` 构造字段；修正 app、structure 测试和 CPU benchmark。`cargo check --locked --workspace --all-targets`、`cargo fmt --all -- --check` 通过。 | 此检查只确认编译和格式；完整 workspace 测试见最终验收。 |
| 2026-11-05 InlineMath 根层与 `.cdoc` 能力边界 | InlineMath 根 TextFlow 需提升到 Paragraph；校验会穿过根 InlineEnvironment 和序列化透明的根 Row，但不进入分数等固定操作数。原根文本格式用例改为真实分数槽位。能力预扫描限定在已知正文/段落/环境/公式载体路径，未知 `extra` 和 Opaque payload 不参与识别；载体能力与 `math.syntax.v1` 按同一模型声明规则校验。`cargo test --locked -p caret-doc --lib`（64 项）、`cargo test --locked -p caret-engine --lib`（456 项）及相关 crate Clippy、Rust fmt 通过。 | Terra/medium 复核发现的误扫未知 payload、对空树/普通 Symbol 过度要求 syntax，以及透明 Row 绕过根层提升问题已修复，需复核修复后完整 diff。数学载体仍按 044 布局门槛只读预览；未运行完整 workspace/native/QML 验证。 |
| 2026-11-06 领域复核与修复 | Terra/medium 复核发现并修复：非 Inline 数学焦点 Mod+Backspace 提前创建投影；不透明/不支持 MathNode 子树中的 vendor `sourceRange` / `sourceMap` 被误拒绝。新增大树、转义预算、失败原子性及未知节点安全预览用例。补齐树原生导航的嵌套文本/数学节点双向往返、Align 空 cell/空行和无效 host/path 失败状态测试。翻译 Terra 检查以锁定 Qt `lupdate` 对照源码提取 160 个 context/source，四个 locale 全量一致，弹窗新增 62 条均为 finished 且占位符一致。 | MathFlow 当前可达的是格式切换与 Font 属性事务，没有独立的通用结构拆分/合并命令契约；不伪造该行为用例。QM 加载由 workspace/native 验证覆盖；最终 Sol/medium 独立只读复审结果见后续验证记录。 |
| 2026-11-06 恢复刷新与数学字素修复 | revision 回绕时，GUI 只发布 pending/target atomics；恢复刷新与冲突保存按 recovery ID 串行，worker 负责重绑和写入。排队快照被新编辑取代时返回 superseded，Rust 为最新修订重排 deadline，Qt 静默结束旧操作；真实 I/O/重绑失败保留已有恢复正文并经文件操作错误通道报告，之后的新编辑可重试。新增 worker gate 前编辑、过期快照重排、真实失败及 callback 乱序用例。修复 Unicode 字素重分段、20K ASCII 快速解析、特殊字符 canonical `\char` 往返、裸撇号 prime 语义及 InlineMath `Mod+Backspace` 降级；长组合字素和转义边界均有行为用例。修复 `math.split_inline` 跨 TextFlow 选区仍显示可用的问题。 | FFI recovery 定向用例、native CTest 9/9、caret-math 100 项及相关 caret-engine 定向用例通过；`cargo format`、Clippy、unused dependency、CMake/QML/layering、clang-tidy/include-cleaner/cppcheck 检查通过。`cargo test --workspace --locked` 全部通过。`cargo quality` 仅在 `cargo deny` 更新 RustSec 数据库时因无法连接 GitHub 失败，不能记作依赖审计通过。Sol/medium 独立完整 diff review 已完成，无阻断项，覆盖率按维护者要求未纳入本轮结论。无性能基线，不宣称性能提升。数学 `.cdoc` 可写、保存/重开及 `DocumentSession` 往返归 044。 |
| 性能与覆盖率暂缓项 | 没有建立数学编辑优化前基线，不宣称性能提升。 | 每次 IME 提交当前复制完整 Session（文档、索引、TextMap 缓存、撤销历史），app 也保存预编辑会话和布局快照；长文档/长历史下的提交延迟尚未测量。降低成本需要跨 engine/app 的延迟历史提交事务，涉及 redo 分支、合并 epoch 与历史预算，本任务未做该重构。本轮按维护者要求暂不测量或判断覆盖率门槛。 |

## 剩余风险与下一步

1. 数学树的 `.cdoc` 解码继续由 043 提供类型化安全预览；数学载体可写能力、真实保存/重开与 `DocumentSession` 往返统一由 044 在布局、绘制、命中及焦点链路接通后验收。
2. 044 负责命令、布局、命中与 IME 预编辑几何；065 负责 TeX 源输入/转换及草稿入口和生命周期设计。
3. 若后续新增通用 MathFlow 环境拆分/合并操作，再按其稳定公开契约补结构身份与失败原子性行为测试。
4. Recovery gate registry 按 recovery ID 保存同步门；`purge_expired` 仅返回数量，无法安全识别应删除的 gate。过期 gate 可能驻留到进程结束；按非原子 `list_entries` 裁剪可能误删仍排队的合法 worker。彻底回收需把删除身份、gate 注册和 worker 排队纳入同一同步协议。

所有失败事务都必须保留有效文档、编辑目标、焦点与历史状态。性能结论须基于代表性输入的可复现测量。
