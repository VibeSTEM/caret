# 044 — M3：公式布局与画布渲染

- 状态：in-progress
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-06

## 目标与背景

把 `caret-math` 的数学盒布局呈现到 Caret 页面画布，并在持久化、布局、绘制、命中和光标路径都可处理公式时接通 `\math`、`\equation`、`\align` 命令。绘制、命中测试、光标几何和选区共用同一 Rust 侧布局快照；Qt 只消费快照并绘制。总体验收由 [011 M3 公式](011-impl-math.md)管理。表格模型与表格画布分别由 045、046 负责。

## 必读

- [数学公式](../architecture/editor/math.md)、[字体选择规范](../architecture/editor/font-selection.md)、[内置字体角色](064-builtin-font-roles.md)、[页面画布与命中测试](../task/027-canvas-and-hit-testing.md)
- [分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现 MathBox 到 Caret 布局快照的适配；包含 glyph ID/位置、规则线、基线、盒尺寸、文本片段映射及必要的字体资源身份。度量使用项目统一的整数表示。Qt 使用既有字形 atlas/QSG 路线，不把 QML 变为公式布局权威。接通正文/块级公式插入候选（`insert.inline_math`、`insert.equation`、`insert.align`）的可用性、候选状态及插入后焦点迁移；Math TextFlow 内部 `math.split_inline` 的结构事务由 043 提供，044 验证其可见几何与可用条件。 当前 `math.split_inline` 仅在选区两端属于同一 TextFlow 时可用；跨 TextFlow 选区暂禁用，直到树事务支持跨流拆分。

覆盖正文中的行内公式、独占行 `Equation`、多行 `Align`，公式字体默认 STIX Two Math，并验证文档 `defaultMathFontFamily` 的统一 `<default>` 标记与诊断回退。标记到角色默认字体的映射由[内置字体角色任务](064-builtin-font-roles.md)和[字体选择规范](../architecture/editor/font-selection.md)定义；本任务只消费数学字体接口。公式文本岛的行内 shaping 和宿主边界遵循[数学公式架构](../architecture/editor/math.md#公式中的普通文本)：复用正文 shaping，不获得宿主段落块级布局。

页面字形快照现提取 glyph、Rule、Line 和 Rectangle；数学图元携带 paint order、颜色和裁剪范围，经 FFI 传到 Qt 场景图，按原顺序绘制。填色盒中的字形、描边与裁剪边界有像素级回归。公式内部命中/光标仍未实现；不得据此启用插入命令或 `.cdoc` 可写门控。

数学 `.cdoc` 的可写能力也归本任务：在公式布局、绘制、命中和编辑焦点链路可用后，启用 `InlineMath`、`Equation`、`Align` 的持久化写入，并验证真实编码/解码、文件保存重开及应用 `DocumentSession` 往返。043 负责模型/schema 与安全预览，不负责该可写门控。

不实现编号公式、字体切换 UI、SVG/PNG-only 可编辑渲染、独立 GPU 基准或大范围段落排版优化。

`Align` 在 `caret-math` 内部临时适配为 `aligned` 矩阵，复用数学后端的列对齐和行距算法；文档模型仍保留 Align 行/单元格身份。Typst 派生的数学 `layout/table.rs` 保留数学片段和 MATH 度量适配，已测量网格的纯几何算法抽取到不依赖 `caret-math` 的 `caret-grid-layout`。045/046 的文档表格布局复用本任务生成的 MathBox 快照，并可在列宽求解与单元格内容重排后复用该几何算法；列表的标记/正文也可用其计算几何，但文档列宽策略、编号、文本重排与分页仍归各自任务。本分支完成 crate 与数学适配器接入；普通表格及列表尚未接入。

## 前置条件与待决策

依赖 043 的稳定公式树、042 的 MathBox API 结论和 027 的画布命中基础；如果后端不能提供编辑命中所需映射，先记录上游最小扩展方案，不用异步 SVG/PNG 绘制绕开契约。

## 实施步骤

1. 定义缓存键和 generation：公式树、字体、字号、语言、缩放/分辨率中哪些影响布局、绘制及 atlas 缓存。
2. 把数学盒输出映射到 Caret 整数坐标与字形/rule 快照；确认公式文本岛混合正文 shaping 时尺寸与基线一致。
3. 接入画布绘制、光标几何、命中测试与选择范围；验证输入更新不会重建未受影响的公式/段落快照。
4. 接通正文/块级 `insert.inline_math`、`insert.equation`、`insert.align` 命令候选和可执行状态；仅在结果可保存、显示、命中与编辑时启用。验证 Math TextFlow 内 `math.split_inline` 的候选只在 Text 分支且两端属于同一 TextFlow 时可用；跨 TextFlow 选区保持禁用。
5. 启用数学 `.cdoc` 可写能力并覆盖 `InlineMath`、`Equation`、`Align` 的真实保存、关闭后重开与 `DocumentSession` 往返；失败保存不得改变原文件或打开会话。
6. 补充具有行为断言的布局/渲染/缓存测试，做性能对比；独立架构与代码评审并处理发现。

## 预计改动

`crates/caret-layout` 数学布局适配，`native` 画布 glyph/rule 绘制，必要的 FFI 快照传输与 Qt 节点绘制；限制为公式内容所需的最小边界。

## 验收标准

- [ ] `InlineMath` 与块级 `Equation`/`Align` 可显示；多行对齐、基线、括号/分数等伸展符号使用数学布局度量。
- [ ] 绘制、命中、光标几何引用同一 generation/几何快照，旧异步结果不能覆盖新布局。
- [ ] 正文/块级公式插入命令仅在结果可持久化、布局、绘制、命中和编辑时成为可执行命令；成功插入后焦点正确，失败保持文档与焦点。Math TextFlow 的 `math.split_inline` 与 043 树事务正确连接；跨 TextFlow 端点时命令不可用。
- [ ] 数学载体在完成布局接入后可由 `CdocFile::to_bytes` 编码、真实写入文件并重新打开；`InlineMath`、`Equation`、`Align` 与 TextFlow 的 ID、属性及未知可选字段往返保持，`DocumentSession` 保存点、dirty 状态及失败原子性正确。不能只把数学能力名加入可写列表而跳过布局门控。
- [ ] 数学 `\text{...}` 英文、中文内容复用正文 TextFlow shaping 与语言行为；已支持的 Run 属性和 InlineEnvironment 生效。Bold/Italic 的嵌套与 `\textmd` / `\textup` 重置分别组合字重、字形，不覆盖另一个样式维度。`\textsf` / `\texttt` 映射到共享 TextFlow 的字体族语义；`sans-serif` / `monospace` 通用族、字体覆盖与 fallback 遵循[字体选择规范](../architecture/editor/font-selection.md)，Math/Code 默认资源遵循[064](064-builtin-font-roles.md)。044 验收这些语义在公式中的 shaping、度量与缺字诊断，不再依赖独立数学文本字体任务。
- [ ] 同一公式快照在缩放与三平台检查中不依赖临时栅格图；页面内编辑没有显著重复重排，性能结果记录基线和测试输入。
- [ ] 无 SVG/PNG-only 交互路径；测试、格式、lint、独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-06 | 本机 Rust：`cargo test --locked -p caret-layout`；`cargo test --locked -p caret-engine layout_view::tests::math_fragment_projection_keeps_content_origin_and_serializes_page_margins` | 分页边界与版心坐标投影行为正确 | 通过：caret-layout 106 项；定向投影测试 1 项。`cargo test --locked -p caret-layout -p caret-engine` 全包尝试未通过：caret-engine 有 2 项 `layout.rs` 测试失败（U+FFFC glyph run、负 inline math 度量用例索引越界），不属于本次分页/视图文件修复，待对应改动完成后复验。 |
| 2026-11-06 | 本机 Rust：`cargo clippy --locked -p caret-layout --all-targets -- -D warnings`；`cargo clippy --locked -p caret-layout -p caret-engine --all-targets -- -D warnings` | 分页 crate 与相关 crate 全目标 lint 通过 | caret-layout 单 crate 通过。跨两个 crate 未通过：`page_glyphs.rs` 的 `append_math_items` 参数过多；`layout/tests.rs` 新增用例触发 5 项 `expect_used`。这些诊断在本次任务文件范围外，待相应实现整合后复验。 |
| 2026-11-06 | 本机 Rust：对本次四个 Rust 文件运行 `rustfmt --edition 2024 --check` | 本次修改格式符合 rustfmt | 通过。仓库级 `cargo fmt --all -- --check` 另报告 `layout/tests.rs` 与 `lib.rs` 的格式差异，未在本次限定范围内修改。 |
| 2026-11-06 | 本机：`cargo test --workspace --locked --offline`、`cargo lint --check`、`cargo format --check` | 页面图元、行内映射、跨层绘制与仓库检查通过 | 通过；包括 native CTest 9/9，像素用例验证不透明填色下字形可见及裁剪边界内的部分描边可见。`cargo quality` 的 advisory DB 检查因沙箱对宿主锁文件只读未完成，详见 068。 |
| 2026-11-07 | 本机 C++：`cargo format --changed --check`；`cargo lint clang-tidy --changed --check`；`cargo lint includes --changed --check`；`cargo lint cppcheck --changed --check`；`CARET_TEST_CTEST_LABELS='^(qml)$' cargo test --locked -p caret-tests --test native` | 图元帧尺寸校验一致；更新 paint order 后 QSG 覆盖顺序正确 | 通过：macOS arm64 的 Qt offscreen 环境中，QML/Shell 相关 CTest 3/3；格式及三项 C++ lint 通过；独立 Terra 领域审查与 Sol final review 通过。未验证 Windows 和 macOS 桌面窗口环境。 |
| — | 数学 `.cdoc` 与 `DocumentSession` 黑盒往返 | 三类公式真实保存、重开、字段保留；失败不覆盖原文件/会话 | 未执行 |
| — | Qt 页面画布人工/组件验证 | 呈现、命中和光标位置吻合 | 未执行 |
| — | 公式布局基准 | 记录输入、构建与缓存条件 | 未执行 |

## 风险与回退

字体缺字、MATH 度量差异、Qt atlas 生命周期或文本 shaping 混排可能造成错位。失败时保留公式源与错误状态，不能退回无法定位和选择的图片对象。性能优化限定在测量证明的热路径。

## 决策与工作记录

- 2026-10-29：从 011 拆出公式布局与画布渲染。
- 2026-11-01：表格模型与交互拆到 045，表格布局和画布集成拆到 046；044 不依赖 012，也不负责单元格公式集成。
- 2026-11-04：明确公式插入命令的候选接入与可用性门控归 044；公式树和内部编辑事务归 043，TeX 源码输入转换由 065 设计。
- 2026-11-06：数学 `.cdoc` 可写能力、真实文件保存/重开及 `DocumentSession` 往返由 044 在布局、绘制、命中链路接通后统一验收；043 保留模型/schema 和只读安全预览职责。
- 2026-11-06：架构重构分支从 Typst 数学表格算法抽取不依赖数学和文档模型的 `caret-grid-layout`；当前仅数学适配器接入。046 文档表格与 051 列表可复用测量后几何，具体列宽求解、正文重排、编号和分页仍归各任务，待实际接入验证。此项不改变 044 总体状态，未完成的命中/光标、命令、持久化与平台验收继续保持未完成。
- 2026-11-06：Line/Rectangle 已加入页面字形快照、FFI 与 QSG 绘制，并按 MathItem paint order 排序；默认前景矩形描边和裁剪边界均有回归。环境页范围现汇总显示公式片段；公式内部命中/光标、插入命令门控、数学 `.cdoc` 可写与三平台验收仍未接入。

## 完成摘要

未完成。
