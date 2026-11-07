# 044 — M3：公式布局与画布渲染

- 状态：done
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-08

## 目标与背景

将 `caret-math` 的数学盒接入 Caret 页面布局、绘制、命中和编辑；在数学结构可保存、可见且可交互后启用 `\math`、`\equation`、`\align`。绘制、命中、光标和选区读取同一 Rust 布局发布版本，Qt 消费快照。总体验收由[011 M3 公式](011-impl-math.md)管理，文档表格模型/画布由 045/046 管理。

## 必读

- [数学公式](../architecture/editor/math.md)、[字体选择](../architecture/editor/font-selection.md)、[内置字体角色](064-builtin-font-roles.md)、[页面画布与命中测试](027-canvas-and-hit-testing.md)
- [分层](../standards/layering.md)、[数值](../standards/numeric-values.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与架构决策

- 覆盖 `InlineMath`、`Equation`、`Align`，公式 glyph/rule/基线/文本映射、命中与编辑几何、插入候选和数学 `.cdoc` 持久化。采用整数布局坐标、既有字形 atlas/QSG 绘制；不使用 SVG/PNG 作为唯一交互表示。
- 普通文本和数学文本共用排版及编辑核心。布局层通过同一 `ParagraphLayouter`/Parley shaping 处理正文与数学 TextFlow；引擎以 `FlowContent<T>` 共用 Run 切分、属性范围变换、样式继承、规范化和 no-op 判断。正文 `EditNode` 与数学 `MathTree`/MathPath 只保留寻址、宿主事务和树回写适配。差异必须由模型语义解释并有成对行为测试，不为表面对称强行合并。
- `caret-layout` 与 `caret-math` 同属 L3，不互相依赖。共享网格几何由无 workspace 依赖的 `caret-grid-layout` 提供；数学 MATH 度量/片段适配留在 `caret-math`。Align 在数学后端内部适配为 aligned 矩阵，文档树保留行、单元格身份。045/046 可复用快照和网格几何，普通表格/列表策略不在本任务实现。
- `caret-doc::InlineTextStyle` 表达解析后的 Bold/Italic/Font 样式增量；`InlineEnvironmentTemplates` 负责从主题快照读取默认 kind 模板与变体，并按模板默认 → 变体 → 实例顺序合并。字体面可用性、真/伪粗斜体及 `<default>` 回退仍属于布局/字体层。正文和数学文本诊断统一经 `textDiagnostics` 发布，带宿主类型和 ID。Math/Code 默认字体资源归 `caret-font-roles`，标记与回退语义见字体规范和 064。
- 不实现编号公式、字体切换 UI、全面段落排版优化，或将数学树强制转换为正文树。`math.split_inline` 的结构事务由 043 提供；044 验证布局/编辑行为，跨 TextFlow 端点目前应禁用。

## 当前进度

已接入公式图元快照、画布绘制、页面命中、焦点/caret/选区/复制、IME 预览、正文与块级公式插入候选，以及 InlineMath/Equation/Align 的真实文件保存重开和 Session 往返。已覆盖旧 generation、防止失败提交污染文档、运行时字体诊断、正文与数学共享文本 shaping/编辑原语。PR #99 仍开放；功能提交 `a484cc2` 的 CI 已通过，task 状态记录提交的 CI 待确认。

复杂数学 bidi、完整鼠标拖选、跨 TextFlow 拒绝反馈及三平台真实桌面交互已拆至 [069](069-math-interaction-and-platform-acceptance.md)，并纳入 [011](011-impl-math.md)。Equation/Align 文档 DTO 来源的首尾及连续 `SoftBreak` 已有 TeX 往返树形回归。数学模板接入正文 shaping、数学文本 shaping、无 shaper 降级路径和编辑命令继承；新能力 ID `caret.env.inline-text-templates.v1` 标识模板 schema，错误 kind/引用/属性由模型校验拒绝。初期不执行旧行内模板语义；未声明新版能力但含旧行内 `definitionId` 引用的文档进入不可编辑、不可保存的 `SafePreview`，声明了新版能力但模板无效时拒绝打开。100k 混合文档 benchmark 已与 PR 原始提交 `838146f` 在同机、同 release 配置对照：首次布局约快 3%，增量段落复用、拖选预览和提交无可检测差异。强制完整重排的首轮 10 样本检测到约 4.7% 回退；后续样本区间较宽的跟进对照未发现显著差异（p=0.09），当前不能确认回退或提升，保留为待 CI/nightly 趋势复测。基准不包含真实 Qt 鼠标轨迹。文本岛 caret/选区查询仍扫描已发布快照，未声称已优化。SafePreview 目前只在 `caret-doc` 存储 API 保留未知主题；应用 `prepare_open` 仍拒绝 SafePreview，实际只读预览仍由 [032](032-unknown-node-preview.md) 跟踪，不计作 044 已交付能力。

## 验收标准

- [x] InlineMath、Equation、Align 的数学布局/绘制接入；多行对齐、基线及伸展符号使用数学度量，Qt 不重复执行领域布局。
- [x] 绘制和交互消费同一发布快照；主要命中、焦点、caret、选择、复制、IME 与失败回滚路径有行为回归。
- [x] 插入命令按可保存、可布局和可编辑状态门控；数学 `.cdoc` 真实编码、写入、重开及 Session 保存点/dirty 状态往返通过，公式/行/单元格 ID、TextFlow/Run 属性和未知可选字段有往返保护。
- [x] 正文和数学 TextFlow 共享 shaping、实例环境样式及可复用编辑核心；粗/斜体分别重置、中文/英文文本与 `sans-serif`/`monospace` 通用族由宿主测试覆盖，模型特有行为保留在适配层。
- [x] 正文/数学缺字及首选字体缺失已接入统一 `textDiagnostics` 到状态栏。
- [x] 完整快照及真实 Rust 增量响应的 Qt ViewModel 诊断更新/清除、通知，正文位于公式前后、普通 U+FFFC 与公式占位符的 Rust 回归，以及 QML 状态栏提示出现/清除均有行为测试。
- [x] 接入文档主题模板 → 变体 → 实例属性的有效样式合并；模型校验覆盖默认映射、kind、引用、变体和属性；正文/数学同一模板解析 API，实例属性最后覆盖，空白 `fontFamily` 仍按未指定处理。
- [x] 无 shaper 数学布局 API 明确说明 `TextFlow` 使用数学字体降级路径，不能获得正文字体族、脚本/语言偏好或 glyph fallback；需要正文外观时调用 shaper API。纯数学/Align 分支也传入模板解析结果。
- [x] `math.split_inline`、换行语义与跨 TextFlow 禁用边界遵循 043 契约；Equation/Align 文档 DTO 来源的首尾及连续 `SoftBreak` 通过 TeX 往返树形断言。跨 TextFlow 的用户反馈和三平台实际行为由 069 验收。
- [x] 复杂数学 bidi、完整鼠标拖选及三平台真实桌面交互缺口已拆至 069，并更新 011 的依赖和子任务表；044 基础 Align 目标 x、短行最近 caret、空 cell 和行边界已有组件行为回归。
- [x] 100k 混合文档首次布局、缓存复用重排、强制完整重排及数学拖选预览/提交已与 PR 原始提交同机对照；区间、显著性和待调查回退已记入性能记录。测量不含真实 Qt 鼠标轨迹。
- [x] 当前 PR CI 通过；本机格式/lint/测试通过，Terra/medium 分域检查及未参与实现的 Sol/medium 完整 diff final review 已完成。Rust 函数覆盖率记录为 84.98%，维护者决定不将此门槛作为本次收尾阻塞。

## 验证记录

2026-11-07，本机已完成：

- `cargo test --workspace --locked`：本次通过（engine 643、app 247、doc 77、math 133；native CTest 9/9）。
- 2026-11-07 后续定向回归：`caret-doc` 样式增量、engine 继承字体、公式占位符误报/漏报以及 `paragraph_delta_projects_and_omits_text_diagnostics` 均通过。Rust 增量视图已直接断言非空诊断投影和空字段省略；Qt ViewModel 另使用实际 Rust 增量响应验证更新和清除。
- Qt Quick 组件回归 `textFontWarningStatusBarTracksLayoutDiagnostics` 通过：状态栏缺字体提示随 ViewModel 诊断显示，并在完整快照清空后隐藏。
- `cargo fmt --all -- --check`、`git diff --check`、`cargo lint --check`：通过（lint 九阶段）。`cargo test --workspace --locked` 内 native/QML 聚合 9/9；Qt Quick zoom、选区和状态投影有交互回归。
- 最新 `cargo coverage`：Rust functions 84.98%、lines 89.18%；行覆盖率通过。维护者决定本次不以函数覆盖率门槛阻塞功能收尾，仍保留该数字供后续 nightly 趋势观察。`cargo coverage native`：产品函数 90.44%、行 85.54%，门槛通过。
- 最新 `cargo quality` 完成格式及 lint 阶段（含 Clippy、依赖/CMake/分层、qmllint、clang-tidy、include-cleaner、cppcheck），在 cargo-deny advisory 数据库锁阶段因沙箱不能写入宿主机 `~/.cargo/advisory-dbs/db.lock` 退出；其余阶段已单独验证。随后在宿主环境运行 `cargo deny check`：advisories、bans、licenses、sources 全通过，只有 deny.toml 中未被当前依赖使用的许可白名单告警。
- 分域复核：Terra/medium 的 engine interaction、layout/math、model/storage 检查均完成；布局复核未发现确定性问题，记录了数学预期值和被测布局共享转换路径的局限。未参与实现的 Sol/medium 已复核完整 `git diff HEAD`，无确定性阻断项；剩余验收限制见上。
- SafePreview 兼容边界经独立复核：缺少新版行内模板能力时仅执行结构校验并进入只读预览；新能力已声明但模板无效时仍拒绝。初期不实现旧行内模板语义。
- Criterion `inline_math_command_typeahead_and_snapshots_in_100k_document`：当前 1.0284–1.1350 ms；紧邻复测 p=0.22，旧参考样本 0.9139–0.9418 ms。因样本轮次/环境不同，不把差异归因于单项改动；等待 CI/nightly 同环境复测。
- Criterion `rust/math_mixed_document_100k`：同机 macOS arm64、Rust 1.99.0、release profile、每组 10 样本，原始 PR 提交 `838146f` 对比当前工作树；首次布局 61.083→58.956 ms（约 −3.5%，p<0.05），增量重排 1.6950→1.7272 ms（无显著差异，p=0.86），强制完整重排 59.876→62.677 ms（约 +4.7%，p<0.05），拖选预览 1.6948→1.7103 μs（无显著差异，p=0.10），选区提交 220.27→218.59 μs（无显著差异，p=0.83）。原提交与当前各 10 样本，1 秒预热、2 秒目标测量；若干较慢分组由 Criterion 自动延长采样。针对完整重排另做无并行编译的跟进对照：原提交均值估计 74.676 ms、当前 65.971 ms，p=0.09；样本区间较宽，未检出显著差异。CPU 型号无法读取；结果不构成因果结论，需用 CI/nightly 持续观察。测量不含 Qt、QML、GPU 和逐像素鼠标事件。
- PR #99 的 CI 曾暴露 Linux 字体代际固定值假设、ShellLoad 超时及渲染器像素相位断言不稳定，均已按实际契约修订；后续拖动距离测试也已改为运行时选择足够远的正文端点。提交 `a484cc2` 对应的 CI 全部通过；末尾 task 状态记录提交将触发新一轮 CI。

069 跨平台/桌面检查由该任务验收。功能提交 `a484cc2` 的 PR CI 已全部通过；本次 task 状态记录和索引同步提交推送后会触发新的 CI，完成后补记结果。每次后续验证只需补记新的命令、结果和未覆盖范围，避免重复记录实现过程。

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

044 已完成：数学页面布局/渲染、正文与数学共享文本排版和编辑核心、主题模板样式解析、存储与主要交互均已接通并通过本机验证、分域复核、Sol/medium final review 和功能提交 CI。维护者接受本次 Rust 函数覆盖率 84.98% 不作为阻塞。复杂 bidi、完整拖选、跨 TextFlow 用户反馈及三平台桌面验收继续由 069 跟踪；SafePreview 的独立建模与转换器另按 032 后续方向推进。当前状态记录和任务索引同步提交推送后触发的新 CI 尚待确认。
