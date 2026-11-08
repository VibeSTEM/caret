# 074 — 数学光标候选与统一仲裁

- 状态：in-progress
- 依赖：071、072
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

所有方向键焦点与选区状态由 `caret-engine::Session` 权威管理，具体实体键经 `Session::run_editor_key` 和 `keyboards/` handler 仲裁。`caret-app` 从同代布局快照生成带 generation 的视觉候选。数学结构（尤其分数、上下标、根式和嵌套槽位）需要 MathLayout 的基线、缩放、节点盒及结构路径来计算 caret 候选；数学树不应另建一份光标状态。

MathFlow 内的普通 TextFlow 应复用正文文本的视觉移动、选区折叠、affinity 和 preferred-x 规则，只适配 `MathTextPosition` 与宿主路径。当前 Qt `math` 子命令 mode 解码已在 072 修复；用户反馈 `\frac` 上下方向键不切换，现有 Engine MathTree 垂直分支没有明确的结构候选提交，需要按统一键盘入口补齐。

MathTree 聚焦时当前不启用输入法。用户提出数学模式请求英文输入方式，同时中文标点（如 `、`）还涉及公式树输入语义与字体 fallback；需评估软提示、Unicode 符号输入和平台限制，不能假设应用能切换宿主 OS 的输入法状态。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[分层规范](../standards/layering.md)、[数学公式架构](../architecture/editor/math.md)、[键盘管理架构](../architecture/interface/keyboard-shortcut-architecture.md)、[键盘路由](../architecture/interface/keyboard-routing.md)、[071 宿主焦点协议](071-host-focus-and-key-routing.md)、[072 空公式宿主占位](072-empty-math-host-placeholder.md)

## 范围与非目标

- 保持单一光标/选区状态与按键仲裁：所有实体方向键最终进入对应 `EditorKey` handler；命令查询、IME 等更高优先级状态在底层几何边缘返回前获得处理。
- MathTree 结构导航使用 MathLayout 几何生成候选，正确跨分数分子/分母、上/下标、根式和 Align cell；不同脚本字号和基线决定 caret 几何高度。
- 普通正文文本沿 Parley 正文路径；MathFlow TextFlow 复用相同文本导航算法，只适配树位置与宿主身份，不复制状态转换规则。
- 审查 math mode 输入法契约：确认可用时采用 Qt 的 Latin 输入提示，明确它不能保证切换 OS 输入法；任意 Unicode/中文符号的插入边界和缺字 fallback 有清楚结果。
- 不让 Engine 复制页面像素模型或依赖 Qt/QML；不重构无证据表明重复的 MathTree 结构语义。

## 前置条件与待决策

- 依赖 071 的 `EditorKey` 单入口与 072 的可命中空宿主节点盒。
- 明确分子/分母与脚本之间的上下移动候选排序、边缘跨结构行为和 shift 扩选规则。
- 确认数学结构编辑时输入法的期望：Latin 提示、IME 禁用范围、罗马字组合输入是否支持，以及中文标点/符号的显式插入方式。
- 使用锁定的 Qt 6.12 评估 `Qt::ImhPreferLatin` / `Qt::ImhLatinOnly` 在项目承载控件上的实际作用和平台差异；提示不是 OS 输入法切换保证。

## 实施步骤

1. 绘制 command line、MathTree、MathText 与正文的方向键输入、候选、handler 和状态迁移图。
2. 添加分数上下导航、脚本高度/基线 caret、MathText mixed-bidi 边缘与命令查询活动时的复现测试。
3. 保留 072 已实现的活动 command line 优先提交实体方向 `EditorKey` 规则，补齐 MathText mixed-bidi/边缘回归；其余数学结构候选经同一 editor-key route 提交。
4. 抽取普通正文与 MathText 可共享的文本候选算法；MathTree 保留确实需要的结构几何适配，不做伪统一。
5. 明确 MathTree 输入法提示和 Unicode 标点/fallback 契约，覆盖不同平台和字体缺字边界；更新架构文档并独立 review。

## 预计改动

`crates/caret-app/src/document_view/navigation.rs`、`crates/caret-engine/src/editing/session/keyboards/{left_arrow,right_arrow,up_arrow,down_arrow,home,end}.rs`、`crates/caret-engine/src/layout.rs` 数学几何查询和对应 Rust/app/native 测试；如输入法策略确认，触及 `crates/caret-app/src/document_view/snapshot.rs`、`native/canvas/` 与 QML 输入法上下文属性。

## 验收标准

- [ ] 命令查询 Math mode 的反斜杠输入、候选选择和方向键不会被底层 MathText/bidi 几何分支提前返回；状态更新完整且无卡键。
- [ ] `\frac` 分子/分母间 Up/Down 可移动；分数、上/下标、根式及 Align cell 的结构导航和 caret 高度反映实际 MathLayout 几何。
- [ ] 普通正文和 MathFlow TextFlow 使用相同文本移动/扩选/preferred-x 规则，并正确映射到各自主机位置。
- [ ] 过期或非法候选拒绝后文档、焦点、选区和撤销历史不变；所有焦点状态仍由 Engine 权威持有。
- [ ] 输入法提示/禁用策略有实测依据和平台限制记录；MathTree 中的 Unicode 符号/标点输入及缺字 fallback 有可见、可恢复行为。
- [ ] 键位处理仍由 `keyboards/` 文件管理；相关 Rust/app/native 行为测试、格式/lint 和独立 final review 通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Engine MathTree/键盘导航测试 | 结构几何正确，过期候选原子拒绝 | 未执行 |
| — | app/native 输入与方向键测试 | Math mode 不锁键，TextFlow 复用正文移动规则 | 未执行 |
| — | 各目标平台输入法提示验证 | 记录提示/禁用策略及 OS/IME 限制 | 未执行 |
| — | 格式/lint 与独立 review | 改动通过仓库门禁 | 未执行 |

## 风险与回退

数学结构没有与普通段落相同的视觉行格；强行复用段落布局会丢失脚本缩放、基线和操作数路径。共享范围限定在 TextFlow 文本候选语义与状态提交，MathTree 继续采用结构布局候选。平台输入法提示不保证系统切换；若 LatinOnly 破坏必要符号输入，则回退为 PreferLatin 并保留应用内显式 Unicode 输入。

## 决策与工作记录

- 2026-11-08：072 的 PR #116 已合并到 main；从同步后的 main 创建 `yuki/task-074-math-caret-routing-and-geometry` 开始实施。

- 2026-11-08：用户确认数学结构需保留上下标相关 caret 几何，普通文本复用标准导航，并补充 `\frac` 上下方向键未切换、数学模式 IME 与中文标点/缺字体验问题。当前 Engine 已有统一 EditorKey 状态仲裁，但 MathTree 纵向候选与输入法策略仍待实现和平台验证。

## 完成摘要

未完成。
