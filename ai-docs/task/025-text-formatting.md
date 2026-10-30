# 025 — 字符与段落格式命令

- 状态：in-progress
- 依赖：009、021、022、024
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

为选择范围和待输入文字提供语义格式操作，不把继承状态、颜色类型或段落属性压成含糊的简单值。

## 必读

- [格式语义](../architecture/editor/text-formatting.md)、[文档模型](../architecture/editor/document-model.md)、[命令](../architecture/editor/commands.md)
- [分层](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

先支持文档默认字体设置和局部 Font EnvironmentNode 命令，再逐步支持字号、粗体/斜体、单线下划线、字色、高亮、字符底纹和基本段落格式。`\firstlineindent` 在当前段落任意光标位置设置首行缩进为 2 个字符宽度，以段落属性存储并进入正常撤销历史；正文 Tab 与 `\doublecharspace` 共用插入两个字符宽空格的编辑动作，`\singlecharspace` 插入一个字符宽空格。正文普通 U+0020 空格按半字宽排版。`\bold`、`\italic`、`\font` 等行内环境命令必须能作用于当前非空文本选区：按字素边界包裹所选文字，保留内容、位置映射和 Undo；空选区时按对应命令定义 pending typing style 或插入点行为。字体族不作为 Run 直接属性：文档默认拉丁族通过命令/API 修改，局部族由 `\font` 命令创建或应用 inline Font EnvironmentNode；字体名从 024 的本机字体目录获取。全局可设置 `defaultCjkFontFamily`，并按 ISO 15924 脚本标签配置细粒度覆盖；CJK 默认包含 `Hani`、`Hira`、`Kana`、`Hang` 和 `Bopo`。数学公式字体由 011 的 `defaultMathFontFamily` 独立设置，不属于正文字体选择器或正文脚本字体候选链；首版默认并仅支持 STIX Two Math。语言继续使用 BCP 47 标签，不能用字体族配置代替语言元数据。当前不做独立字体选择窗口或新工具栏控件。粗体/斜体范围由行内 EnvironmentNode 包裹，格式操作注册到命令系统，不增加环境工具按钮。实现文档全局格式设置、主题选择和默认 Run/Paragraph 样式；主题加载及显式更新走命令/API，主题快照更新失败必须保留旧快照和样式引用。块级 EnvironmentNode 的创建、嵌套、选择和删除由 010 通过命令与结构快捷键处理；将命令应用于非空文本选区并包装成块环境由 [057](057-block-environment-selection-commands.md) 定义，本任务不重复实现。当前环境背景高亮不由格式命令设置，随光标位置派生。保留嵌套继承，并在移除格式环境时 unwrap 子内容。保留继承状态，格式查询表达 uniform/mixed/unavailable；空选区更新 pending typing style。一次确认对应一个事务。

不包含完整 Word 样式兼容、所有 underline 枚举、透明色或工具栏视觉细节。

## 前置条件与待决策

021 数据模型、022 选择/命令、024 shaping 契约稳定。颜色及主题引用语义以 020 之后的架构为准。

## 实施步骤

1. 定义属性的继承、显式关闭和清除行为。
2. 实现默认字体命令和 Font EnvironmentNode 命令，基于 024 字体目录提供命令补全，不增加字体选择窗口。
3. 实现其他范围格式命令、inline environment wrap/unwrap 和格式查询。
4. 覆盖混合选区、空选区、空段、嵌套粗斜体、unwrap 边界与属性合并。
5. 独立评审并复验。

## 预计改动

`caret-doc/src/model/inline.rs` 的行内环境节点、`run.rs` 与 `paragraph.rs` 的直接属性载体；`caret-engine` 中彼此独立的行内环境树操作和 Run/Paragraph 直接属性操作模块、格式查询 DTO 和命令测试。命令类型和注册仍由统一入口管理，格式执行器按行为边界分开。当前不预建空模块。

## 验收标准

- [ ] 字色/高亮/底纹/页面色/工作区色语义互不覆盖。
- [ ] 默认字体命令修改文档全局默认；局部字体命令创建/应用 Font EnvironmentNode；Run 不直接保存字体族；不新增字体选择窗口或工具栏控件。
- [ ] 文档默认字体设置支持默认拉丁字体、CJK 默认字体、按 Unicode Script 短标签覆盖字体、脚本内 BCP 47 语言覆盖；混合文字能在 shaping 边界选择对应字体，并在 glyph 缺失时使用系统回退。
- [ ] 不同字体族的 Font 环境局部嵌套覆盖；同族范围合并/压平并按 020 保留节点 ID。
- [ ] `\firstlineindent` 在当前段落任意光标位置将首行缩进设为 2 em；Tab 与 `\doublecharspace` 共用插入两个字符宽空格的编辑动作，`\singlecharspace` 插入一个字符宽空格。首行缩进作为段落属性参与 Parley 换行、光标几何、选区绘制、撤销/重做和 `.cdoc` 往返。
- [ ] `\bold`、`\italic`、`\font` 等行内命令可作用于当前选中文本；按字素边界切分并包裹目标范围，保留未选内容、方向与位置映射。混合格式选区按[格式语义](../architecture/editor/text-formatting.md#选区与待输入格式)统一应用；空选区切换 pending typing，移动光标后按上下文重算；交叉范围按 020 的保序树化规则切分；只有状态、`definitionId`、`props` 和语义上下文兼容的同类型相邻环境才合并。行为测试覆盖主题/外层粗斜体的局部显式关闭、不同模板嵌套时保留已有属性优先级、空段标记格式、拆分、合并和 Undo ID 恢复。优先级命令测试固定全局字号 14 pt、Section 默认模板字号 18 pt 且实例无覆盖时环境内结果为 18 pt；实例变体/props 覆盖和环境未设置该属性时的全局/主题回退也分别断言。不同有效树形按正文、计算格式和位置映射断言，不以树完全相同作为通过条件。
- [ ] 自动颜色与显式白色可区分；未知格式保留或报告降级。
- [ ] 失败不部分修改；一个格式操作交给 018 作为一个撤销单元。纯绘制属性不使排版缓存失效；度量属性只失效其影响范围。
- [ ] 外部主题变化不自动应用；显式更新成功后才替换快照并推进对应样式代际，失败保留原快照、引用和可见效果。
- [ ] 独立评审和受影响测试通过。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 属性查询与范围命令测试 | 全选/混合/空选区语义正确 | 未执行 |
| — | 序列化往返 | 格式和未知值按决议保留 | 未执行 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine`、`cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings` | 行内格式命令保留文本与选区；混合/显式关闭、跨段选区、待输入与撤销规则正确；连续键入合并兼容环境 | 引擎测试 157 项通过；Clippy 通过。格式行为测试 14 项覆盖 `\\bold` 命令候选接受、显式关闭、跨段范围、PendingStyle Undo/Redo、粗斜体连续键入合并与高亮、IME 预编辑/提交、组合附加符及未编辑 ID 保留。任务其余格式能力仍未实现。 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine`、`cargo test --locked --offline -p caret-engine virtual_environment_exit`、`cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 字体环境与默认字体 API 的选区、待输入、IME、嵌套、撤销/重做、失败原子性和持久化行为正确 | 引擎测试 167 项通过；虚拟块环境外输入/IME 提交 3 项通过；Clippy、格式检查和差异空白检查通过。字体行为测试覆盖局部选择、跨段反向选区、嵌套覆盖、连续输入合并、IME 预览/提交、默认字体撤销/重做、同偏移不同亲和性空选区、超深拒绝、失败原子性、未知字段和 JSON 往返。独立审查发现并修复虚拟环境外首段输入丢失 pending font 的问题，输入和 IME 共用待输入样式应用逻辑。`\\font` 字体目录候选及命令面板执行仍未接通，不能视为该命令已完成。 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine font_command_opens_unicode_family_picker_and_applies_selected_family`、`cargo test --locked --offline -p caret-app font_command_snapshot_exposes_unicode_picker_mode_and_utf16_cursor` | `\font` 从本机字体目录搜索 Unicode 字体名；命令输入/候选快照位置正确，选中后提交 Font 环境 | 引擎命令行为测试通过；app 快照测试通过，验证字体模式前缀及中文查询的 UTF-16 光标位置。完整验证待执行。 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine -p caret-app`、`cargo clippy --locked --offline -p caret-engine -p caret-app --all-targets -- -D warnings`、`cargo format --check`、`cargo lint --check`、`git diff --check` | 字体族命令候选和快照跨层一致；Unicode 查询可用；所有支持语言翻译、QML 展示和相关模块检查通过 | 引擎 168 项、app 182 项测试通过；Clippy、格式、跨语言 lint（含 QML/C++）及差异检查通过。`\font` 接受后打开本机字体族候选模式，Unicode 字体名可前缀筛选；接受候选后通过现有事务创建/应用 Font 环境。完整 025 仍有字号、颜色、段落格式等范围未完成。 |
| 2026-10-30 | `cargo fmt --all -- --check`、`cargo test --locked --offline --workspace`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`、`git diff --check`；跨层行为测试含 CTest | 空格命令名、Tab 行为、U+0020 半字宽排版、空格塑形缓存及命令/IME 路由符合契约；workspace 回归通过 | 格式、workspace 测试、Clippy 与差异检查通过；native CTest 8/8 通过。覆盖 Rust 命令与布局、Qt IME 取消后恢复输入、QML 加载/格式及资源加载。命令别名统一为 `\singlecharspace` / `\doublecharspace`；普通 U+0020 以半个 em 校准，显式字符宽空格按一个 em 校准，Tab 插入两个字符宽空格。100k 密集空格单段实测另见下行；尚未复测百万字多段文档或统计分配量。Qt 输入法 reset 后可能已排队的旧 commit 没有 generation 过滤，当前测试仅验证取消后新 commit 可正常提交，平台事件顺序仍需实机验证。完整 025 仍未完成。 |
| 2026-10-30 | `cargo bench --locked --offline -p caret-layout --bench layout_criterion -- 'space_dense_100k' --sample-size 10 --warm-up-time 1 --measurement-time 2`；优化 bench（macOS arm64） | 约 100k ASCII 字符、密集普通空格，检查空间校准后的 100k 单段布局耗时 | Release 下 99,996 个 ASCII 字符布局中位数 16.477 ms，95% 区间 16.350–16.804 ms，5.79 MiB/s；Criterion 与本机已有基线相比未检测到显著变化（p=0.92）。这是单机基线，不等于证明相对未修改代码无退化；尚未测量百万字多段文档和分配量。 |
| 2026-10-30 | `cargo test --locked --offline -p caret-tests --test document first_line_indent_round_trips_through_cdoc_storage` | 新增首行缩进在 `.cdoc` 编码、重新打开后仍保留 | 通过；断言保存 JSON 和重新打开后的文档均保留 `firstLineIndentChars: 2`。 |

## 风险与回退

把 unset 转成 false 会破坏继承。使用明确值类型及迁移测试。

## 决策与工作记录

- 2026-10-25：从原 008 编辑范围拆出格式命令。
- 2026-10-25：用户决定粗体/斜体的混合选区采用统一应用；规则维护在格式语义文档。
- 2026-10-25：用户决定合并同类型粗体或斜体环境，不保留冗余嵌套。
- 2026-10-25：用户决定空选区的粗体/斜体命令切换 pending typing；移动光标后按新位置上下文重新计算，输入时局部切分/合并格式环境。
- 2026-10-25：文档级全局格式与主题选择/更新归入本任务；更新失败保留旧快照，缓存按格式影响分类失效。
- 2026-10-26：用户决定正文默认字体按文字体系而非语言拆分：默认拉丁字体供英语等拉丁文字共用，CJK 有通用默认字体并可按 `Hani` 等脚本覆盖。
- 2026-10-29：修订数学字体决策：保留 `defaultMathFontFamily` 作为数学字体的稳定扩展入口，011 首版默认并仅支持内嵌 STIX Two Math；不增加多字体切换 UI。
- 2026-10-30：明确行内环境命令必须支持作用于已选文本；块环境命令的选区包装由 057 单独定义，避免与行内字素范围操作混为一体。
- 2026-10-30：开始实现行内粗体/斜体命令；新增 Rust Session 格式事务、通用 `\bold`/`\italic` 候选分发、QML 标签及四种语言文案。发现关闭格式必须置于既有环境链内侧，已通过显式 `enabled: false` 行为测试固定；PendingStyle 状态纳入会话撤销历史且不改变文档修订代际。默认字体、Font 命令、其他字符和段落属性及格式查询尚未实现。

- 2026-10-30：提交 `6a539c6` 完成行内粗体/斜体首批能力。随后开始补 Rust 字体设置 API：文档默认拉丁字体、局部 Font 环境、跨段范围及待输入样式；`\\font` 的 FontCatalog 候选和命令面板参数交互尚未实现，字体选择 UI 仍归 056。
- 2026-10-30：字体范围处理对同段选区按稳定 Paragraph ID 直接定位，避免常见编辑路径扫描并排序整份 Story；跨段选区仍按结构顺序收集段落。文档最大嵌套深度由 `caret-doc` 导出单一常量，超限字体环境操作在安装修改前拒绝。全局节点数上限仍由文档模型校验，Session 尚无增量节点计数；接近 2,000,000 节点时的编辑增长边界需与既有编辑操作一并评估，当前不宣称该极限已覆盖。
- 2026-10-30：独立 final review 发现块环境外虚拟停靠点的首段输入绕过 pending 样式；已在段落安装前统一应用格式并增加直接输入与 IME 行为断言。复核另发现嵌套深度少计段落自身一层，已对齐常规段落定位规则修正。最终复核中。
- 2026-10-30：将 `\font` 接入本机字体目录候选。命令模式接受 `\font` 后切换到 Unicode 字体族查询模式；Rust 持有候选与 UTF-8 光标，QML 快照额外提供 UTF-16 光标索引。动态字体候选不进入 QML 的隐藏全量文本测量器，候选列表继续使用虚拟化显示。
- 2026-10-30：空格命令采用 `\singlecharspace` 与 `\doublecharspace`，避免 `\onespace` / `\twospace` 被误解为空格个数；正文 Tab 等同于插入两个字符宽空格。普通 U+0020 的目标宽度为半个 em，并在塑形布局中按字体、脚本和相关样式校准；显式字符宽空格按一个 em 校准。单段 100k 密集空格基线见上表；百万字多段布局及分配量仍待测量。
- 2026-10-30：独立 final review 首轮发现首行缩进持久字段缺少 `.cdoc` 往返行为测试；新增 `first_line_indent_round_trips_through_cdoc_storage`，定向测试通过，Sol medium 复核 approve。Qt reset 后旧 IME commit 的到达顺序属于未确认平台限制，未用可能吞掉有效输入的“忽略下一次 commit”闩锁规避。Reviewer 指出首次校准样式会额外构建一次 Parley layout，字体候选查询会扫描/克隆匹配项并序列化候选快照；单段 100k 密集空格未见相对本机已有基线的显著差异，但没有未修改代码的对照或分配统计，百万字文档与大字体目录成本仍需后续测量。

## 完成摘要

未完成。
