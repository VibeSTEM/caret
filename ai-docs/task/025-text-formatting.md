# 025 — 字符与段落格式命令

- 状态：in-progress
- 依赖：009、021、022、024
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-30

## 目标与背景

提供基于文档语义的字符和段落格式操作。格式状态必须保留继承、显式关闭、混合值和待输入样式等区别；Rust 文档/编辑会话是状态权威，Qt/QML 只传递操作并显示 Rust 投影。

本任务按能力逐步交付，目前已完成字体基础、粗体/斜体、单线下划线、首行缩进和字符宽空格。字号、颜色/底纹及其余段落格式仍未完成，因此任务保持 `in-progress`。字体目标和优先级以[字体选择规范](../architecture/editor/font-selection.md)为唯一权威；格式属性归属以[文字格式语义](../architecture/editor/text-formatting.md)为唯一权威。

## 必读

- [格式语义](../architecture/editor/text-formatting.md)、[文档模型](../architecture/editor/document-model.md)、[命令](../architecture/editor/commands.md)
- [字体选择规范](../architecture/editor/font-selection.md)、[字体映射目录](../architecture/editor/font-defaults/README.md)
- [分层](../standards/layering.md)、[数值与单位](../standards/numeric-values.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 完成文档字体默认值、局部 Font 环境及字号、粗体/斜体、下划线、颜色/底纹和基本段落格式命令。
- 行内环境格式对非空选区按字素边界操作，保留文本、选区方向、位置映射和 Undo；空选区使用 Rust Session 的 pending typing style。Run/Paragraph 直接属性与环境格式不建立重复存储来源。
- `\firstlineindent` 将当前段落首行缩进设为两个字符宽度，任意光标位置可用并作为段落属性持久化；正文 Tab 插入两个字符宽度空格，`\singlecharspace` / `\doublecharspace` 分别插入一个/两个字符宽度空格。普通 U+0020 空格按半个 em 排版。
- `\font` 和 `\defaultfont` 在命令输入中提供字体族与 Script/Script+Language 选择；候选与目标规则复用 Rust 字体服务。命令弹窗的后续交互和布局完善由[059](059-command-font-picker.md)跟进。
- `defaultCodeFontFamily` 是独立文档字段，`"<default>"` 表示平台等宽字体；命令输入使用该字体角色，Rust shaping 为未来代码块接入 `FontRole::Code`。代码块节点不属于本任务。
- 块环境命令由 010 管理；将块命令应用于选区由[057](057-block-environment-selection-commands.md)管理。当前环境高亮由光标位置派生，不属于格式命令。
- 不实现完整 Office 样式兼容、全部下划线类型、透明色、工具栏格式控件或代码块节点；数学字体 UI 继续由 011 决定，首版使用 STIX Two Math。

## 前置条件与待决策

021 文档模型、022 编辑会话/选区、024 shaping 契约已稳定。实现新的属性前，先在格式语义文档确定其存储载体、继承优先级、查询状态、布局失效范围和 `.cdoc` 能力标记；不要在本任务另造映射规则。

## 实施步骤

1. 按属性确定唯一存储语义和用户可见命令，区分环境格式与 Run/Paragraph 直接属性。
2. 在 Rust Session 中实现事务、位置映射、pending style、查询及失败原子性；命令统一注册在命令入口。
3. 接入布局、画布和 QML 投影；绘制属性不应无故触发排版，度量属性只使受影响范围失效。
4. 为格式操作补行为测试、持久化测试和必要的 Qt/QML 可观察行为测试；代表性大文档变化需用实际耗时/分配量验证。
5. 更新规范和任务状态，按验证规范完成独立评审与复验。

## 预计改动

`crates/caret-doc` 的 Run/Paragraph 属性和文档设置，`crates/caret-engine/src/editing/` 的命令、行内环境及段落格式事务，`crates/caret-layout` 的 shaping/度量，必要的 FFI、Qt 画布与 QML 命令投影及其行为测试。具体改动按能力边界拆分；不为缩短文件机械拆模块或 crate。

## 验收标准

### 已完成

- [x] `\bold` / `\italic` 使用行内 `EnvironmentNode`；支持选中文本、混合选区统一应用、显式局部关闭、空选区 pending typing、跨段格式、IME、Undo/Redo、环境拆分/合并和节点 ID 保留。行为测试覆盖格式树与可见文本/位置语义。
- [x] 文档字体默认值按 Script 与 Script+Language 目标由 Rust 文档设置持有；`\defaultfont` 可键盘导航目标层级并提交设置，`\font` 可查询本机字体并将所选字体应用为局部 Font 环境。字体缺失保留请求并由 shaping fallback 处理；UI 不扫描目录。
- [x] `defaultCodeFontFamily` 默认值、校验、`.cdoc` 持久化和快照投影已接通；代码 shaping 使用显式字体或平台 Monospace，保持代码角色等宽空格语义。实际代码块节点尚未实现。
- [x] `\underline` 使用 Run 直接属性，支持选区、混合范围、pending typing、软换行、IME 和 Undo/Redo；Rust/Parley 提供装饰几何，Qt 使用独立 QSG 装饰层，行框不变。
- [x] `\firstlineindent`、Tab、`\singlecharspace`、`\doublecharspace` 与普通 U+0020 半字宽排版语义已接通；首行缩进参与布局、光标/选区几何、撤销/重做和 `.cdoc` 往返。
- [x] 上述已交付能力通过对应 Rust、跨层、QML 加载/格式和持久化测试；字体弹窗初始实现通过构建与加载检查。

### 待完成

- [ ] 实现字号调整及其直接属性、单位校验、继承/混合状态查询、布局失效和持久化语义。
- [ ] 实现字色、高亮、字符底纹等格式，并严格区分自动色、显式颜色、页面色、工作区色；未知值按格式语义保留或报告降级。
- [ ] 实现剩余基本段落格式命令与默认 Run/Paragraph 样式；明确它们和文档主题/环境模板的优先级及缓存失效范围。文档主题加载/显式更新只有纳入本任务实际实现时才验收，失败必须保留原快照和引用。
- [ ] 补足新增格式的失败原子性与单操作单 Undo 单元验证；绘制属性不使排版缓存失效，度量属性只使影响范围失效。
- [ ] 补充下划线装饰跨 FFI/Qt 的像素级端到端断言；命令字体弹窗的边缘定位、滚动和鼠标行为由 059 验收，不阻塞本任务既有命令状态/API。
- [ ] 对新增行为和长文档代表输入完成性能验证；有性能改善结论时提供可复现耗时或分配量对比，不以代码更短或缓存增加作为证据。
- [ ] 所有剩余实现的命令、文档字段和受支持 locale 文案齐全；完整独立 final review 通过后才能将任务标记 `done`。

## 验证计划与结果

| 日期 | 环境 / 检查 | 结果 |
| --- | --- | --- |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine -p caret-app`；对应 Clippy、格式和 `cargo lint --check` | 字体命令、候选快照、Unicode 查询、跨段选区、pending/IME、Undo/Redo 及相关跨层检查通过；测试数随后续能力变更，具体以命令输出为准。 |
| 2026-10-30 | `cargo test --locked --offline --workspace`、`cargo lint --check`、`cargo format --check`；native CTest | workspace 测试与检查通过，native CTest 8/8。覆盖 underline 序列化/软换行、空格与首行缩进、QML 加载/格式及相关 Qt 输入行为。 |
| 2026-10-30 | `.cdoc` 首行缩进往返定向测试、代码字体模型/持久化/投影及 `caret-layout` 测试 | 定向测试通过；代码 FontRole 显式族、Monospace fallback 与代码空格 advance 有布局级测试，代码块端到端未覆盖（节点不存在）。 |
| 2026-10-30 | `cargo bench --locked --offline -p caret-layout --bench layout_criterion -- 'space_dense_100k' --sample-size 10 --warm-up-time 1 --measurement-time 2`（macOS arm64） | 单段 99,996 个 ASCII 字符、密集空格：中位数 16.477 ms，95% 区间 16.350–16.804 ms，5.79 MiB/s；与本机已有基线相比无显著差异（p=0.92）。该结果不是未修改代码对照，也不代表百万字多段文档或分配量。 |
| 2026-10-30 | Sol medium 独立只读 final review（当前已实现代码 diff） | 通过，无 P1/P2。记录尚待实测字体弹窗边缘/滚动/鼠标行为、underline 跨 FFI/Qt 像素断言和大目录/长文档分配量；因此不代表整个 025 已验收。 |

以上记录只说明对应检查当时的结果；后续变更须重跑受影响验证。未记录的本机手工操作不视为自动化验收证据。

## 风险与回退

把未设置值压成 `false` 会破坏继承；颜色若将自动值和显式颜色合并会丢失文档语义。每项新格式必须保留明确值类型、能力标记及原子事务；发生失败时保持旧文档和有效快照不变。缓存失效按属性影响分类，不能让 QML/C++ 维护第二份格式状态。

## 决策与工作记录

- 2026-10-25：粗体/斜体混合选区统一应用；同类冗余环境合并；空选区命令切换 pending typing，移动插入点后按新位置重算上下文。
- 2026-10-29：首版数学字体保留稳定字段 `defaultMathFontFamily` 并使用 STIX Two Math，不增加数学字体切换 UI。
- 2026-10-30：格式范围选择使用 Inline EnvironmentNode；Run/Paragraph 直接属性只用于其语义允许的属性。块环境选区命令交由 057。
- 2026-10-30：字体选择器只使用 Script 与 Script+Language 层级；`defaultCodeFontFamily` 独立于正文脚本字体，代码 shaping 复用该字段并使用 Monospace 角色。
- 2026-10-30：空格命令命名为 `\singlecharspace` / `\doublecharspace`；正文 Tab 插入两个字符宽空格，普通 U+0020 的目标 advance 为半个 em，按字体/脚本校准。
- 2026-10-30：命令弹窗分为普通命令与字体专用 QML 展示，共用弹窗 frame；命令字体选择器后续交互优化转入 059，Settings 专用字体布局由 056 等待独立设计。
- 2026-10-30：独立审查修正虚拟环境外 pending 样式、字体嵌套深度、软换行格式状态、首次编辑能力标记和长文档几何精度问题；当前已完成能力的限制见验收与验证记录。

## 完成摘要

已交付字体基础、粗体/斜体、单线下划线、首行缩进和字符宽空格。字号、颜色/底纹及其余段落格式、underline 像素端到端验证和完整 025 final review 尚未完成；任务继续保持 `in-progress`。
