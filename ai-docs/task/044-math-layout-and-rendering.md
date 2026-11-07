# 044 — M3：公式布局与画布渲染

- 状态：in-progress
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-07

## 目标与背景

把 `caret-math` 的数学盒布局呈现到 Caret 页面画布，并在持久化、布局、绘制、命中和光标路径都可处理公式时接通 `\math`、`\equation`、`\align` 命令。绘制、命中测试、光标几何和选区共用同一 Rust 侧布局快照；Qt 只消费快照并绘制。总体验收由 [011 M3 公式](011-impl-math.md)管理。表格模型与表格画布分别由 045、046 负责。

## 必读

- [数学公式](../architecture/editor/math.md)、[字体选择规范](../architecture/editor/font-selection.md)、[内置字体角色](064-builtin-font-roles.md)、[页面画布与命中测试](../task/027-canvas-and-hit-testing.md)
- [分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现 MathBox 到 Caret 布局快照的适配；包含 glyph ID/位置、规则线、基线、盒尺寸、文本片段映射及必要的字体资源身份。度量使用项目统一的整数表示。Qt 使用既有字形 atlas/QSG 路线，不把 QML 变为公式布局权威。接通正文/块级公式插入候选（`insert.inline_math`、`insert.equation`、`insert.align`）的可用性、候选状态及插入后焦点迁移；Math TextFlow 内部 `math.split_inline` 的结构事务由 043 提供，044 验证其可见几何与可用条件。 当前 `math.split_inline` 仅在选区两端属于同一 TextFlow 时可用；跨 TextFlow 选区暂禁用，直到树事务支持跨流拆分。

覆盖正文中的行内公式、独占行 `Equation`、多行 `Align`，公式字体默认 STIX Two Math，并验证文档 `defaultMathFontFamily` 的统一 `<default>` 标记与诊断回退。标记到角色默认字体的映射由[内置字体角色任务](064-builtin-font-roles.md)和[字体选择规范](../architecture/editor/font-selection.md)定义；本任务只消费数学字体接口。公式文本岛的行内 shaping 和宿主边界遵循[数学公式架构](../architecture/editor/math.md#公式中的普通文本)：复用正文 shaping，不获得宿主段落块级布局。

页面字形快照现提取 glyph、Rule、Line 和 Rectangle；数学图元携带 paint order、颜色和裁剪范围，经 FFI 传到 Qt 场景图，按原顺序绘制。填色盒中的字形、描边与裁剪边界有像素级回归。TextIsland 已有页面快照命中入口，覆盖 InlineMath、Equation 和 Align；文本焦点、caret、选区及 IME 几何已接入 Session/Qt 交互快照。公式结构/空槽命中、插入命令门控与数学 `.cdoc` 可写仍未完成，因此不得启用插入命令或数学 `.cdoc` 写入。

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

`caret-math` 的中立文本岛测量契约、`caret-engine` 对数学与正文塑形器的编排、`caret-layout` 可独立复用的 Parley 文本岛塑形入口，以及公式内部映射/缓存/页面字形快照。`caret-math` 与 `caret-layout` 同为 L3，不得互相依赖；正文 Parley 结果由 engine 按同一布局发布版本持有并供绘制和交互读取。native/FFI 仅在需要传输文本岛真实字体资源或新交互几何时修改，不能在 Qt 重复 shaping 或路径语义。

## 验收标准

- [ ] `InlineMath` 与块级 `Equation`/`Align` 可显示；多行对齐、基线、括号/分数等伸展符号使用数学布局度量。
- [ ] 绘制、命中、光标几何引用同一 generation/几何快照，旧异步结果不能覆盖新布局。
- [ ] 正文/块级公式插入命令仅在结果可持久化、布局、绘制、命中和编辑时成为可执行命令；成功插入后焦点正确，失败保持文档与焦点。Math TextFlow 的 `math.split_inline` 与 043 树事务正确连接；跨 TextFlow 端点时命令不可用。
- [ ] 数学载体在完成布局接入后可由 `CdocFile::to_bytes` 编码、真实写入文件并重新打开；`InlineMath`、`Equation`、`Align`、`MathFlow`、行与单元格的 `NodeId`，以及 TextFlow/Run 属性和未知可选字段往返保持，`DocumentSession` 保存点、dirty 状态及失败原子性正确。当前 schema 没有 TextFlow 自身的 ID；交互映射以公式宿主、MathPathStep 结构路径和布局发布版本作为稳定身份。不能只把数学能力名加入可写列表而跳过布局门控。
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
| 2026-11-07 | 本机 C++ 复核：`cargo format --changed --check`；`cargo lint --check`；`cargo test --locked -p caret-tests --test native` | FFI 图元适配器校验、像素相位补偿和画布绘制通过 | 通过：变更文件格式检查、完整 lint；macOS arm64 Qt offscreen CTest 9/9。测试构造异常 `PageGlyphFrame`，经生产使用的 `to_variant_map` 验证负宽/负高错误键与有效水平线；像素相位用例比较 applied/unapplied 两种实际渲染结果，补偿移除后断言会失败。Terra 分领域审查和 Sol 对 C++ 子集的只读复核未发现确定性运行时问题。Sol 指出用例尚未独立校验补偿方向与精确幅值；未做变异测试。Windows 与 macOS 桌面窗口环境未验证。 |
| 2026-11-07 | 本机 Rust：`cargo format --changed`；`cargo test --locked -p caret-math`；`cargo check --locked -p caret-engine`；`cargo clippy --locked -p caret-math --all-targets -- -D warnings` | MathFlow/TextFlow、嵌套环境、操作数、矩阵行和 Align cell 均携带可区分稳定路径；透明 Row 范围包含后代 | 通过：caret-math 113 项。覆盖空流 identity box、嵌套 `EnvironmentChild`、操作数/矩阵行路径、Align cell scope、空操作数、零 Run TextFlow 序列化不变，以及双层 Row 在脚标/重音布局中保持绘制几何并聚合非空范围。engine 检查和 clippy 通过。独立 Terra 复查无确定性问题。空流仍是零尺寸锚点，尚未成为可点击命中区域；未测布局性能基线。 |
| 2026-11-07 | 本机 Rust：`cargo fmt --all -- --check`；`git diff --check`；`cargo lint --check`；定向测试 catalog identity、TextIslandLayouter 与 caret-engine 小字号/零宽 underline | 字体缓存跨目录代际隔离；公式文本岛保留脚本浮点字号及空格宽度；零推进装饰不阻断页面字形快照 | 通过：格式、差异空白及 9 阶段仓库 lint 通过；跨 catalog identity 1 项、layout 文本岛 5 项、小字号 1 项、zero-advance underline page-glyph 1 项均通过。Run.text 内嵌 mandatory break 尚未处理。 |
| 2026-11-07 | 本机 Rust：`cargo test --locked -p caret-layout -p caret-engine` | 当前数学文本岛与页面快照全量用例通过 | 通过：caret-layout 119 项、caret-engine 551 项；包括浮点字号和空格缩放、跨 catalog 代际复用、小字号上标与零 advance underline 栅格化。 |
| 2026-11-07 | 本机 Rust：`cargo test --locked -p caret-layout text_island_hit_and_selection_geometry_stay_on_grapheme_boundaries` | 文本岛复用 Parley 的视觉命中/光标/选区几何并保持组合字素与 RTL 边界 | 通过：组合字素 Latin 与 Arabic RTL 的行首尾命中返回字素边界；光标几何和完整选区矩形有效。该底层接口已接入数学宿主/MathPathStep 与页面快照命中；结构/空槽、Session/Qt 焦点及可见光标仍待完成。 |
| 2026-11-07 | 本机 C++/Qt 与 Rust：`cargo format --check`、`git diff --check`、`cargo lint --check`、`cargo test --locked -p caret-tests --test native`、`cargo test --locked -p caret-layout selection_geometry` | 字形适配边界与画布可表示域一致；折叠选区保持为空且不能落在扩展字素内部 | 通过：完整 lint 9 阶段通过；Qt offscreen native CTest 9/9；正文与 TextIsland 选区回归 2 项通过。Terra 指出并修复适配器曾接受画布拒绝的超范围页码/图像宽度；Sol final review 指出并修复正文与 TextIsland 折叠空选区错误扩成字素高亮，`0..0` 返回空矩形、组合字素内部 `1..1` 拒绝。独立复审后无其他确定性问题。未验证 Windows 或 macOS 桌面窗口环境。 |
| 2026-11-07 | 本机：`cargo test --locked -p caret-layout -p caret-engine -p caret-app`；`cargo fmt --all -- --check`；`git diff --check`；`cargo lint --check` | 公式文本焦点、选择与复制、IME 预览、完整快照、失败原子性及受影响 crate 回归 | 通过：app 222 项、engine 555 项、layout 119 项；格式、diff 检查与 lint 9 阶段均通过。回归覆盖 Equation 点击与 caret 几何一致、Shift 选择/复制、完整布局刷新保留选区、删空后继续输入、短预编辑替换时 preview/current/interaction caret 一致、composition 发布失败回滚，以及最小字号上标 shaping。未覆盖数学鼠标拖选、混排双向视觉边缘、跨 Align 行上下导航、相邻空岛命中竞态的专用用例和性能基线。 |
| 2026-11-07 | 本机 Rust：`cargo test --locked -p caret-layout -p caret-engine -p caret-app`；`cargo fmt --all -- --check`；`git diff --check` | 正文与空行内公式竞争命中；公式选区 anchor 视觉侧历史恢复；行内对象后的组合字素边界 | 通过：新增源范围 grapheme fence、公式/正文可点击性及折叠选区历史回归；Terra 分域复核确认状态同步修复。未覆盖真实 bidi 下的按键折叠、公式鼠标拖选与性能基线。 |
| 2026-11-07 | 本机 C++/Qt 与 Rust：`cargo format --check`、`git diff --check`、`cargo test --locked -p caret-tests --test native` | 新增 TextIsland 数学页面命中入口可编译；C++ 字形适配边界及 Qt 集成回归通过 | 通过：格式与 diff 检查通过，native CTest 9/9；当前 C++ diff 的 Terra 分域检查未发现确定问题。首次构建发现 `annotate_hit` 闭包误放在相邻的正文命中方法，已移入数学命中方法并由完整 native 构建/测试复验。未运行完整 Rust workspace 测试；Session/Qt 数学焦点尚未接入。 |
| 2026-11-07 | 本机 Rust/C++：`cargo test --locked -p caret-engine text_hit`、`cargo test --locked -p caret-engine math_softbreak_and_opaque_nodes_require_text_shaping_even_with_empty_text_flow`、`cargo format --check`、`git diff --check`、`cargo lint --check`、`cargo test --locked -p caret-tests --test native` | InlineMath/Equation/Align 数学文本命中使用已发布几何；空 TextFlow 边界节点不会令公式布局失败；全仓检查保持通过 | 通过：数学文本命中 4 项（含 InlineMath、Equation、Align）；SoftBreak/Opaque 真实 Equation 布局回归 1 项；仓库 lint 9 阶段、格式与 diff 检查通过；native CTest 9/9。Terra 的 C++ 分域检查和 Sol 对最终完整 diff 的只读 review 未发现剩余确定性问题。Sol review 发现并修复了无文本流时的 scope 选择失败，以及新增英文解释注释；测试覆盖的桌面环境为本机 offscreen，未验证 Windows/macOS，也未做性能基线。 |
| 2026-11-07 | 本机 Rust/C++：`cargo fmt --all -- --check`；`git diff --check`；`cargo test --locked -p caret-layout -p caret-engine -p caret-app`；`cargo lint --check` | 行内公式零宽热点与正文点击仲裁正确；交互/布局回归、全仓 lint 通过 | 通过：三个 crate 的测试全部通过；格式/diff 检查通过；lint 九阶段通过，包括 Clippy、依赖/分层、native/QML 构建、qmllint、clang-tidy、include-cleaner、cppcheck。Sol/medium 最终复审批准；复审建议的热点优化已完成：普通正文点击和正宽公式命中不再查询正文竞争几何，零宽公式仍进行竞争仲裁。当前 C++ 生产改动已单独严格审查并由 clang-tidy、include-cleaner、cppcheck、native CTest 9/9 验证；本轮无新增 C++ 源码改动。未做长文档性能基线，跨平台桌面与复杂 bidi/拖选仍待验证。 |
| 2026-11-07 | PR #99 Linux CI 失败修复：`cargo test --locked -p caret-ffi ffi_layout_snapshot_contains_rust_page_and_paragraph_layout`；`cargo clippy --locked -p caret-ffi --all-targets -- -D warnings`；格式与 diff 检查 | FFI 布局快照验证进程唯一字体代际，不依赖固定计数 | 首轮 CI 发现旧断言固定要求 `fontGeneration == 1`，与现已采用的跨 catalog 进程唯一代际不符。测试改为断言其为非零 u64，并保留同一运行时快照稳定性检查；定向测试、caret-ffi Clippy、格式与 diff 检查通过。第二轮 CI 的 Rust 全包与其他检查通过，但 Ubuntu `Qml.ShellLoad` 因通用 20 秒上限超时（20.02 秒）；多数用例通过。该聚合测试单独设为 45 秒，macOS 本地 QML CTest 3/3（ShellLoad 10.55 秒）通过；等待新一轮 CI。 |
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
- 2026-11-07：核对 `TextFlow`/`Run` schema 后修正身份契约：二者没有独立 NodeId；公式交互映射使用宿主/流 scope、类型化 MathPathStep、Run 下标和同一布局发布版本。MathLayout 节点盒已输出稳定 scope/path/run 下标，TextFlow/Run 不再伪造 NodeId；Run language/props 已传入共享正文 Parley shaping，页面快照通过 sidecar token 读取同一批字形几何。
- 2026-11-07：布局适配器递归保留 `Content`、`EnvironmentChild`、`Operand` 结构步骤；TextFlow Run 的盒子以作用域 MathFlow ID、路径和 Run 下标识别，`MathNodeBox.id` 对无持久 ID 的文本片段为空。矩阵行沿用根作用域与操作数路径；Align 每个 cell root 单独作为 scope。透明 Row 输出聚合子盒后的节点范围，空 MathFlow/TextFlow 输出稳定零尺寸锚点，后续命中层仍须为其建立可用的插入位置。
- 2026-11-07：严格复查修正透明 Row marker：marker 不参与原子属性、间距或斜体修正，按源片段顺序插入节点盒以保留前序遍历；容器范围聚合支持 MathFlow 与透明 Row，并按活动祖先深度遍历。双层非空 Row 回归覆盖脚标与重音，确保盒范围包含后代且包裹不改变 MathItem 几何。该改动降低最坏扫描复杂度，但未建立前后性能基准。
- 2026-11-07：TextFlow/Parley 分层设计复核建议由 `caret-engine` 编排 `caret-math` 与 `caret-layout`：`caret-math` 提供中立文本塑形请求/结果契约，engine 传入共享字体目录和正文设置并保留 Parley sidecar，数学绘制项以本次布局 token 引用 sidecar，命中/光标/选区反向映射到同一结果。不得建立 L3 同级依赖，也不能退回数学字体逐字符塑形。跨 Run/wrapper 的组合 shaping、RTL/fallback、Run 样式/语言、MathAlphabet 边界、SoftBreak/Opaque 区隔、字体目录代际失效和绘制顺序已有实现与回归。含文本岛的布局缓存只在文档源、样式、宽度、字体目录代际及 session 匹配时复用。仍待实现/验证：公式内部命中、光标和选区；Run 内嵌 mandatory break 的定义与处理；真实数学字体选择与对应栅格资源原子接线。
- 2026-11-07：共享 backend 为序列化与布局分离身份元数据；LaTeX 序列化不构造 MathPathStep 向量，零 Run TextFlow 的布局占位不进入序列化结果。
- 2026-11-07：对 #98 C++ 适配器及画布回归做严格复核。将 Rust 帧转换拆为可注入的适配入口，直接验证负尺寸错误键和合法零尺寸线段；paint-order 覆盖像素坐标加入 QSG 对已栅格化字形的 pixel-phase 补偿。
- 2026-11-07：再次严格检查当前分支 C++ diff，范围包括 glyph-frame 适配器、ViewModel 请求边界、QSG 消费端、CMake 注册和画布测试。Terra 分领域审查与 Sol 独立 final review 均未发现确定的生产回归；格式、clang-tidy、include-cleaner、cppcheck 检查通过，focused Qt 像素用例通过。像素相位回归只证明切换 applied 标记会改变画面，未证明 X/Y 各自的方向和精确幅度；尝试用真实字形质心验证时受亚像素采样影响而不稳定，已撤回该断言。完整 native 聚合曾通过；实验性断言存在期间的后一次聚合因 ShellLoad 总时限失败，不代表最终代码结果。未验证 Windows 或 macOS 桌面窗口环境。
- 2026-11-07：最终完整 diff 复查发现字体目录 generation 只在单个目录内递增，导致两个新 catalog 都为 1 时可错误复用旧 Parley/数学文本 sidecar；改为进程唯一代际并补跨实例断言。另修复零 advance 下划线 run 将整页字形提取判为无效的问题，并补含 U+200B 的公式页面提取与栅格化回归。脚本浮点字号现在直接沿 TextIslandLayouter 的 Parley 样式范围传递，不再经文档 centipoint 量化；空格校准目标也使用相同派生字号并以字号位值失效。回归验证合法 1 centipoint 正文字号的数学 glyph 与上标文本字号分别保持 0.01 pt 与小于 0.009 pt，普通与全角空格 advance 随 6/6.135 pt 浮点字号缩放。完整 review 还发现 Run.text 中强制换行字符 schema 合法但文本岛拒绝，语义处理仍待决定。
- 2026-11-07：为公式文本岛交互接通页面命中、Session 焦点安装、caret 投影、同一 Parley 布局中的视觉左右与 Home/End 导航、Shift 选择、选区几何/复制和数学 IME composition underline。完整布局快照与轻量交互快照均读取数学文本几何；空文本岛在删除最后一个字素后保留 caret，允许继续输入。IME preview 发布失败会恢复提交前 Session 与预览状态。视觉边缘按纯逻辑首尾选择退出方向，视觉侧变化会切断撤销合并组。焦点与树位置校验保持 generation、字体代际、宿主、路径及字素边界约束。
- 2026-11-07：Terra/medium 分域复核发现并推动修复空文本岛选择几何、0.01pt 以下合法数学上标字号投影、纯 RTL 边缘逻辑方向、完整快照的数学选区投影、视觉选区折叠条件、anchor 路径 no-op、IME preview caret 与发布失败原子性，以及数学 anchor 视觉侧历史恢复。行内公式对象源范围现在给正文 Parley grapheme segmentation 增加原子 fences；正文命中只排除真实 InlineMath source range，避免把普通 U+FFFC 当作对象。正文与空公式热点分别有应用层回归。仍未实现数学文本鼠标拖选；文本岛布局目前为单行，上下键跨 Align 行需由公式/页面几何提供目标；混排双向视觉边缘、真实 bidi 的按键折叠和跨 Align 行上下导航仍待实现/验证。数学 caret/选区查找仍遍历已发布快照，缺少代表性长文档基线，未声称性能提升。
- 2026-11-07：Sol/medium 最终只读复审批准当前完整 diff。按复审建议将正文竞争区域判断延迟到存在零宽数学文本命中时，避免普通正文点击及正宽公式点击复制字素边界并查询选区几何；零宽公式与正文竞争行为保持原样。三 crate 全量测试、仓库格式/diff 检查和九阶段 lint 复验通过。该改动减少了常规点击的额外工作，但未建立长文档基线，也不声称量化性能提升。
- 2026-11-07：PR #99 Linux CI 首轮发现 FFI 快照测试仍假设 `fontGeneration` 从 1 起步；随着字体 catalog 代际改为跨实例唯一，该假设不成立。改为解析根级字段、检查非零代际并保留布局快照稳定性断言；本地 FFI 定向测试与 Clippy 通过。第二轮 CI 发现 Ubuntu `Qml.ShellLoad` 在通用 20 秒限时到期；Rust 包、Clippy、格式、CMake、分层、QML lint、C++ lint 与 math backend 均通过。仅对该聚合测试设 45 秒上限；macOS 本地相关 QML CTest 3/3、格式与九阶段 lint 通过，新一轮 CI 待完成。

## 完成摘要

未完成。
