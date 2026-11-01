# 025 — 字符与段落格式命令

- 状态：in-progress
- 依赖：009、021、022、024
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-11-01

## 目标与背景

提供基于文档语义的字符和段落格式操作。格式状态必须保留继承、显式关闭、混合值和待输入样式等区别；Rust 文档/编辑会话是状态权威，Qt/QML 只传递操作并显示 Rust 投影。

本任务交付字体基础、粗体/斜体、单线下划线、首行缩进和字符宽空格，并收尾验证跨层绘制、环境解除行为及长文档格式性能。原计划中尚未实现的字号、字色/底纹、其他段落属性与命名样式已拆到 060–063，不再扩大本任务的实现范围。字体目标和优先级以[字体选择规范](../architecture/editor/font-selection.md)为唯一权威；格式属性归属以[文字格式语义](../architecture/editor/text-formatting.md)为唯一权威。

## 必读

- [格式语义](../architecture/editor/text-formatting.md)、[文档模型](../architecture/editor/document-model.md)、[命令](../architecture/editor/commands.md)
- [字体选择规范](../architecture/editor/font-selection.md)、[字体映射目录](../architecture/editor/font-defaults/README.md)
- [分层](../standards/layering.md)、[数值与单位](../standards/numeric-values.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 收尾已交付的文档字体默认值、局部 Font 环境、粗体/斜体、下划线、首行缩进和字符宽空格能力。
- 行内环境格式对非空选区按字素边界操作，保留文本、选区方向、位置映射和 Undo；空选区使用 Rust Session 的 pending typing style。Run/Paragraph 直接属性与环境格式不建立重复存储来源。
- `\firstlineindent` 将当前段落首行缩进设为两个字符宽度，任意光标位置可用并作为段落属性持久化；正文 Tab 插入两个字符宽度空格，`\singlecharspace` / `\doublecharspace` 分别插入一个/两个字符宽度空格。普通 U+0020 空格按半个 em 排版。
- `\font` 和 `\defaultfont` 在命令输入中提供字体族与 Script/Script+Language 选择；候选与目标规则复用 Rust 字体服务。命令弹窗的后续交互和布局完善由[059](059-command-font-picker.md)跟进。
- `defaultCodeFontFamily` 是独立文档字段，统一标记 `"<default>"` 解析为内置 JetBrains Mono；命令输入使用该字体角色，Rust shaping 为未来代码块接入 `FontRole::Code`。代码块节点不属于本任务。数学字段也使用同一标记并解析为内置 STIX Two Math。
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

### 实施与验收状态

- [x] `\bold` / `\italic` 使用行内 `EnvironmentNode`；支持选中文本、混合选区统一应用、显式局部关闭、空选区 pending typing、跨段格式、IME、Undo/Redo、环境拆分/合并和节点 ID 保留。行为测试覆盖格式树与可见文本/位置语义。
- [x] 文档字体默认值按 Script 与 Script+Language 目标由 Rust 文档设置持有；`\defaultfont` 可键盘导航目标层级并提交设置，`\font` 可查询本机字体并将所选字体应用为局部 Font 环境。字体缺失保留请求并由 shaping fallback 处理；UI 不扫描目录。
- [x] `defaultCodeFontFamily` 默认值、校验、`.cdoc` 持久化和快照投影已接通；代码 shaping 使用显式字体或内置 JetBrains Mono，保持代码角色等宽空格语义。实际代码块节点尚未实现。
- [x] 历史实现：`\underline` 曾以 Run 直接属性保存，并接通选区、pending typing、软换行、IME、Undo/Redo、Rust/Parley 装饰几何和 Qt QSG 绘制；对应历史行为测试通过。该存储结果不代表当前目标架构已经实现。
- [ ] 按[行内与块级环境适配](../architecture/editor/document-model/inline-and-block-environments.md)统一下划线载体、范围高亮与 Mod+Backspace 写回；补齐 Run 边界、InlineMath 边界、键盘上下文和失败原子性验证。`.cdoc` 尚未稳定发布，不要求兼容早期开发分支格式。
- [x] `\firstlineindent`、Tab、`\singlecharspace`、`\doublecharspace` 与普通 U+0020 半字宽排版语义已接通；首行缩进参与布局、光标/选区几何、撤销/重做和 `.cdoc` 往返。
- [x] 上述已交付能力通过对应 Rust、跨层、QML 加载/格式和持久化测试；字体弹窗初始实现通过构建与加载检查。
- [x] 下划线装饰跨 Rust FFI/Qt Quick 场景图的像素级行为测试通过；命令字体弹窗的边缘定位、滚动和鼠标行为由 059 验收，不阻塞本任务既有命令状态/API。
- [x] Mod+Backspace 在行内嵌套环境中逐层解除且保留内容；IME 预编辑和命令输入上下文中不误解除环境，行为由 Rust Session 测试覆盖。
- [x] 代表性长文档格式操作完成实测并记录绝对耗时。该结果是当前实现的测量，不是优化前后对比，不据此声称性能提升。
- [x] 基础能力的实现、持久化、失败原子性与 Undo 行为经完整独立 final review。

字号、绘制格式、段落格式和文档命名样式分别由 060、061、062、063 跟进，不属于本任务范围。

## 验证计划与结果

| 日期 | 环境 / 检查 | 结果 |
| --- | --- | --- |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine -p caret-app`；对应 Clippy、格式和 `cargo lint --check` | 字体命令、候选快照、Unicode 查询、跨段选区、pending/IME、Undo/Redo 及相关跨层检查通过；测试数随后续能力变更，具体以命令输出为准。 |
| 2026-10-30 | `cargo test --locked --offline --workspace`、`cargo lint --check`、`cargo format --check`；native CTest | workspace 测试与检查通过，native CTest 8/8。覆盖 underline 序列化/软换行、空格与首行缩进、QML 加载/格式及相关 Qt 输入行为。 |
| 2026-10-30 | `.cdoc` 首行缩进往返定向测试、代码字体模型/持久化/投影及 `caret-layout` 测试 | 定向测试通过；代码 FontRole 显式族、Monospace fallback 与代码空格 advance 有布局级测试，代码块端到端未覆盖（节点不存在）。 |
| 2026-10-30 | `cargo bench --locked --offline -p caret-layout --bench layout_criterion -- 'space_dense_100k' --sample-size 10 --warm-up-time 1 --measurement-time 2`（macOS arm64） | 单段 99,996 个 ASCII 字符、密集空格：回归斜率估计 16.477 ms，95% 置信区间 16.350–16.804 ms，5.79 MiB/s；与本机已有基线相比无显著差异（p=0.92）。该结果不是未修改代码对照，也不代表百万字多段文档或分配量。 |
| 2026-10-30 | `cargo bench --locked --offline -p caret-app --bench document_view_input_criterion -- 'underline_one_paragraph_in_100k_document' --sample-size 10 --warm-up-time 1 --measurement-time 2`（macOS arm64，release） | 100,499 Unicode 字符、500 段多语言文档中格式化中间段并生成快照：10 样本；Criterion 回归斜率估计 1.5899 ms，95% 置信区间 1.5742–1.6245 ms；2 个 outlier。文档准备和选区在计时外；命令行 begin/type/accept、布局更新与快照在计时内；没有历史对照组，不声称性能提升。复现说明见[性能基准说明](../../tests/performance/rust/README.md)。 |
| 2026-10-30 | `QT_QPA_PLATFORM=offscreen target/native/debug/qml/caret_shell_load_test underlinedTextRendersTheRustFfiDecorationInTheQtCanvas`；`ctest --test-dir target/native/debug -R '^Qml\.ShellLoad$' --output-on-failure` | 新增的 Rust FFI → ViewModel → QSG 像素断言通过；渲染帧使用真实 Rust 下划线几何，但移除字形图像与位置，避免字形抗锯齿造成假阳性。相关 QML/C++ shell 测试通过。 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine -p caret-app`；`cargo fmt --all -- --check`；Clang Format；`git diff --check` | Mod+Backspace、编辑会话和 app 相关测试通过；详细数量以最终命令输出为准。 |
| 2026-10-30 | Mod+Backspace 修订后：`cargo test --locked --offline -p caret-engine -p caret-app`、`cargo clippy --locked --offline -p caret-engine -p caret-app --all-targets -- -D warnings`、格式检查和 `git diff --check` | 191 个 caret-app 测试与 186 个 caret-engine 测试通过；Clippy、Rust 格式和 diff 检查通过。 |
| 2026-10-30 | `cargo lint --check` | 9 个阶段全部通过：Clippy、依赖检查、CMake 格式/lint、分层、native 构建/QML 元数据、qmllint、clang-tidy、include-cleaner 和 cppcheck。 |
| 2026-10-30 | Sol medium 独立只读 final review（本次完整实现 diff） | 通过，无需修复项。确认 Mod+Backspace 的树变更与历史测试、下划线 FFI/QSG 装饰绘制测试的隔离方式，以及 Criterion 计时范围和统计值。限制：未重跑 benchmark；像素行为尚未覆盖取消格式和多 DPR/缩放组合；性能数据只代表 100k 文档中单段格式操作。 |
| 2026-10-30 | review 后修复：`cargo test --locked --offline -p caret-engine -p caret-app`；构建 `caret_shell_load_test` 并运行 `ctest --test-dir target/native/debug -R '^Qml\.ShellLoad$' --output-on-failure`；`cargo lint --check` | caret-app 192 项、caret-engine 189 项通过；ShellLoad 通过；9 阶段 lint 全部通过。新增覆盖行内环境深度溢出原子拒绝、重复 Bold 切换不增长环境深度、跨段格式失败原子性与 Undo 历史、待输入格式不污染保存状态，以及同步重入时 IME 取消只发出一次事件。修复后未重跑性能基准，既有数据仍是单次测量。 |
| 2026-10-30 | Sol medium 独立只读 final review（修复后完整分支 diff） | 通过，无新增 findings。复核 Bold/Italic 原位更新同类环境、嵌套深度检查、PendingStyle 保存状态、IME 同步重入保护、Criterion 统计术语和新增回归测试。限制：评审未重跑测试或 GUI；多 DPR/缩放组合未确认。 |

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
- 2026-10-30：复核发现并修复四项问题：Bold/Italic 更新选区内已有同类环境并限制新增嵌套深度；pending 样式保留撤销历史但不推进文档保存状态 ID；IME 取消事件增加同步重入保护；性能表将 16.477 ms 正确标为回归斜率估计。

## 完成摘要

已交付字体基础、粗体/斜体、历史 Run 下划线实现、首行缩进和字符宽空格；当前下划线载体与环境适配仍待完成。补齐了跨 FFI/Qt Quick 的像素行为测试、Mod+Backspace 语义测试及 100k 多语言文档格式性能测量，并通过独立 final review。其他格式能力已拆分为 060–063。
