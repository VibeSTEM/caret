# 044 — M3：公式布局与画布渲染

- 状态：in-progress
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-07

## 目标与背景

将 `caret-math` 的数学盒接入 Caret 页面布局、绘制、命中和编辑；在数学结构可保存、可见且可交互后启用 `\math`、`\equation`、`\align`。绘制、命中、光标和选区读取同一 Rust 布局发布版本，Qt 消费快照。总体验收由[011 M3 公式](011-impl-math.md)管理，文档表格模型/画布由 045/046 管理。

## 必读

- [数学公式](../architecture/editor/math.md)、[字体选择](../architecture/editor/font-selection.md)、[内置字体角色](064-builtin-font-roles.md)、[页面画布与命中测试](027-canvas-and-hit-testing.md)
- [分层](../standards/layering.md)、[数值](../standards/numeric-values.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与架构决策

- 覆盖 `InlineMath`、`Equation`、`Align`，公式 glyph/rule/基线/文本映射、命中与编辑几何、插入候选和数学 `.cdoc` 持久化。采用整数布局坐标、既有字形 atlas/QSG 绘制；不使用 SVG/PNG 作为唯一交互表示。
- 普通文本和数学文本共用排版及编辑核心。布局层通过同一 `ParagraphLayouter`/Parley shaping 处理正文与数学 TextFlow；引擎以 `FlowContent<T>` 共用 Run 切分、属性范围变换、样式继承、规范化和 no-op 判断。正文 `EditNode` 与数学 `MathTree`/MathPath 只保留寻址、宿主事务和树回写适配。差异必须由模型语义解释并有成对行为测试，不为表面对称强行合并。
- `caret-layout` 与 `caret-math` 同属 L3，不互相依赖。共享网格几何由无 workspace 依赖的 `caret-grid-layout` 提供；数学 MATH 度量/片段适配留在 `caret-math`。Align 在数学后端内部适配为 aligned 矩阵，文档树保留行、单元格身份。045/046 可复用快照和网格几何，普通表格/列表策略不在本任务实现。
- `caret-doc::InlineTextStyle` 只表达 InlineEnvironment 实例的 Bold/Italic/Font 样式增量，不解析主题模板/变体，也不决定字体面或合成方式；字体可用性、真/伪粗斜体及 `<default>` 回退属于布局/字体层。模板优先级尚未接入本次共用接口。正文和数学文本诊断统一经 `textDiagnostics` 发布，带宿主类型和 ID。Math/Code 默认字体资源归 `caret-font-roles`，标记与回退语义见字体规范和 064。
- 不实现编号公式、字体切换 UI、全面段落排版优化，或将数学树强制转换为正文树。`math.split_inline` 的结构事务由 043 提供；044 验证布局/编辑行为，跨 TextFlow 端点目前应禁用。

## 当前进度

已接入公式图元快照、画布绘制、页面命中、焦点/caret/选区/复制、IME 预览、正文与块级公式插入候选，以及 InlineMath/Equation/Align 的真实文件保存重开和 Session 往返。已覆盖旧 generation、防止失败提交污染文档、运行时字体诊断、正文与数学共享文本 shaping/编辑原语；共享逻辑及其宿主差异有回归测试。PR #99 仍开放，最近一次已知 CI 轮次仍有 Ubuntu ShellLoad 测试修订待验证。

仍未完成：真实桌面和 Windows/Linux 验证；跨 Align 行上下导航、混排 bidi 边缘行为及完整拖选轨迹；数学 `SoftBreak` 的布局换行与 TeX 序列化为空格之间的语义差异；环境模板/变体的有效样式合并；长文档/大公式候选和拖选性能的代表性测量。文本岛 caret/选区查询仍扫描已发布快照，未声称已优化。最新 100k 逐键基准为 1.0284–1.1350 ms，高于旧样本 0.9139–0.9418 ms；与紧邻复测差异不显著（p=0.22），须由新 CI/nightly 对照后再判断趋势。

## 验收标准

- [x] InlineMath、Equation、Align 的数学布局/绘制接入；多行对齐、基线及伸展符号使用数学度量，Qt 不重复执行领域布局。
- [x] 绘制和交互消费同一发布快照；主要命中、焦点、caret、选择、复制、IME 与失败回滚路径有行为回归。
- [x] 插入命令按可保存、可布局和可编辑状态门控；数学 `.cdoc` 真实编码、写入、重开及 Session 保存点/dirty 状态往返通过，公式/行/单元格 ID、TextFlow/Run 属性和未知可选字段有往返保护。
- [x] 正文和数学 TextFlow 共享 shaping、实例环境样式及可复用编辑核心；粗/斜体分别重置、中文/英文文本与 `sans-serif`/`monospace` 通用族由宿主测试覆盖，模型特有行为保留在适配层。
- [x] 正文/数学缺字及首选字体缺失已接入统一 `textDiagnostics` 到状态栏。
- [x] 完整快照及真实 Rust 增量响应的 Qt ViewModel 诊断更新/清除、通知，正文位于公式前后、普通 U+FFFC 与公式占位符的 Rust 回归，以及 QML 状态栏提示出现/清除均有行为测试。
- [ ] 接入文档主题模板 → 变体 → 实例属性的有效样式合并；空白 `fontFamily` 当前作为未指定解释、编辑命令拒绝新写入，仍需正文/数学嵌套字体成对回归。
- [ ] 明确公开无 shaper 数学布局 API 对含 TextFlow 的限制，或让它显式拒绝无法按正文字体/fallback 排版的内容。
- [ ] `math.split_inline`、换行语义和跨 TextFlow 禁用条件与 043/模型契约一致；剩余语义差异有明确决策和回归。
- [ ] 三平台与真实桌面交互验证完成；复杂 bidi、数学拖选和跨 Align 行导航达到产品要求，或明确拆分为后续任务并更新 011。
- [ ] 用代表性长文档/大公式测量插入候选、布局重用、拖选和编辑性能；解释相对基线的变化并记录配置。
- [ ] 当前 PR CI、格式/lint/测试与覆盖率门槛通过；按验证规范完成 Terra/medium 分域检查和未参与实现的 Sol/medium 完整 diff final review。

## 验证记录

2026-11-07，本机已完成：

- `cargo test --workspace --locked`：本次通过（engine 640、app 243；native CTest 9/9）。
- 2026-11-07 后续定向回归：`caret-doc` 样式增量、engine 继承字体、公式占位符误报/漏报以及 `paragraph_delta_projects_and_omits_text_diagnostics` 均通过。Rust 增量视图已直接断言非空诊断投影和空字段省略；Qt ViewModel 另使用实际 Rust 增量响应验证更新和清除。
- Qt Quick 组件回归 `textFontWarningStatusBarTracksLayoutDiagnostics` 通过：状态栏缺字体提示随 ViewModel 诊断显示，并在完整快照清空后隐藏。
- `cargo fmt --all -- --check`、`git diff --check`、`cargo lint --check`：通过（lint 九阶段）。native/QML 聚合测试 9/9、QML 子集 3/3；Qt Quick zoom、选区和状态投影有交互回归。
- `cargo coverage`：Rust functions 85.09%、lines 89.23%；`cargo coverage native`：产品函数 90.41%、行 85.56%。
- Criterion `inline_math_command_typeahead_and_snapshots_in_100k_document`：当前 1.0284–1.1350 ms；紧邻复测 p=0.22，旧参考样本 0.9139–0.9418 ms。因样本轮次/环境不同，不把差异归因于单项改动；等待 CI/nightly 同环境复测。
- PR #99 的 CI 曾暴露 Linux 字体代际固定值假设、ShellLoad 超时及渲染器像素相位断言不稳定，均已按实际契约修订；后续拖动距离测试也已改为运行时选择足够远的正文端点。修订后的 CI 结果尚未确认。

未完成的跨平台/桌面检查和本次最终独立评审不得用上述本机结果替代。每次后续验证只需补记新的命令、结果和未覆盖范围，避免重复记录实现过程。

## 风险与回退

- 正文与数学文本必须保持共享核心；若模型差异无法由稳定适配边界表达，先补等价/反例用例再调整抽象，禁止复制第二套通用算法。
- 性能测量出现回退时，先用同机、同构建和相同输入复测并定位热点；不能以增加缓存或缩短代码宣称优化。
- 持久化、布局或交互任一阶段失败时，命令保持不可用，文档、焦点和原文件保持原状。
- `InlineTextStyle` 是可扩展的文档样式增量，不是完整 resolved style；新增字段须由正文、数学、编辑状态及无 shaper API 明确处理，避免消费者逐字段应用时静默漏接。

## 决策与工作记录

- 2026-10-29：创建任务，目标为公式页面布局、渲染与交互。
- 2026-11-06：完成页面图元投影、字体/坐标边界及基础数学文本岛验证；早期失败项已随相关实现修正。
- 2026-11-07：统一 L3 网格几何边界；接通数学绘制、持久化和主要编辑路径；按维护者要求持续合并正文/数学的文本排版与编辑逻辑。`FlowContent<T>` 承担通用变换，正文与数学保留宿主寻址/事务适配。验证和审查细节以本节“验证记录”及 PR #99 CI 为准。
- 2026-11-07：共享文本样式和 `textDiagnostics` 跨 Rust/Qt/QML 接通；最新完整实现记录见验证记录。工作树后续改动仍需按标准重新评审。

## 完成摘要

未完成。跨平台/真实桌面、剩余交互与换行语义、代表性性能复测、修订后 CI 和最终独立评审通过后，再更新状态及 011 索引。
