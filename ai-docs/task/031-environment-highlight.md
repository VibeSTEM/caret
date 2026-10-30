# 031 — 当前环境背景高亮

- 状态：done
- 依赖：010、027、033
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-30

## 目标与背景

让用户能通过画布背景识别光标所在的环境。该高亮是视图状态，不写入文档、主题快照或 `.cdoc`。环境内文字仍由常规字形层绘制，现有文字颜色和底纹不被替换。

009 已实现命令输入提示色；010 提供块环境结构范围，块级背景高亮及分页绘制已接入。行内 `Bold` 环境高亮现已接入并完成相关测试与独立评审，本任务状态为 `done`。光标在 `Bold` 节点内容中时，只衬出其实际覆盖的字形范围，不能把整段 Paragraph 染色，也不能只高亮光标所在字素。

## 必读

- [010 结构实现](010-impl-structure.md)、[027 画布与命中](027-canvas-and-hit-testing.md)
- [文档模型](../architecture/editor/document-model.md)、[光标与选区职责](../architecture/interface/caret.md)
- [分层](../standards/layering.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

块环境高亮从 Rust 当前结构位置和同代布局快照取得稳定 ID 与分页矩形，由 Qt Quick scene graph 绘制独立圆角背景层。行内高亮由 Rust 根据当前光标位置解析最内层 inline `EnvironmentNode` 的稳定 ID 及 UTF-8 半开文本范围，再复用布局/Parley 选区几何产生按视觉行分片的背景矩形；范围须覆盖环境内全部文字，并支持软换行和 bidi。Qt 只绘制 Rust 快照，不遍历文档树、不自行塑形或推导范围。

环境高亮跟随当前光标位置：进入/离开 `Bold`、`Italic` 或 `Font` 边界时即时切换。默认、浅色、深色主题为行内和块级环境提供独立 GUI token；命令输入提示仍使用独立 token。所有几何须与当前布局 generation 对齐。

块级和行内高亮都位于纸张之上、选区和字形之下，不重绘正文、不重新计算布局，也不改变文档 generation 或序列化内容。两者复用同一 QSG 环境高亮绘制层，分别提供 Rust 计算的几何和独立颜色 token。命令提示由 QML 控件背景绘制，因为它是锚定插入点的交互弹出控件；它与正文高亮共享视觉约定，不共用跨渲染层绘制代码。分层原因和场景顺序见[画布与交互背景](../architecture/interface/canvas-highlighting.md)。

不包括环境创建/样式编辑、主题定制 UI，或把高亮写入文档格式。

## 设计提案与待确认边界

- `Bold` 是首个行内验收样例。Bold 会使塑形选择粗体字体样式/实际字体 face；高亮宽度必须从该样式已经排版的文本几何获得，不能以普通字体估算。后续 `Italic`、`Font` 通过同一范围解析和绘制路径，不按 kind 复制实现。
- 行内环境矩形来自布局使用的 UTF-8 文本范围与选区几何；不能把整段矩形当作行内范围。
- 采用“最内层环境独占当前高亮”：光标进入 Section 中的 Bold 时只显示 Bold 行内高亮；离开 Bold 后恢复块级 Section 高亮。行内环境优先于其祖先块环境，不叠加两种环境底色；命令提示可与当前环境高亮并存。
- 光标在行内环境边界时沿用既有 `Affinity` 规则决定是否处于环境内；不能用视觉左右键方向直接替代逻辑亲和性。
- 与 055 的关系：行内高亮使用 Parley/Caret 已计算的实际文本范围几何，纵向填充共享行框；OpenType BASE 对齐不应改写行框，因此 031 无需等待 055 才能实现。055 必须让 glyph、caret、selection 等共享同一基线结果；BASE 完成后再用含 Bold/粗体 face 的中英混排用例做跨任务回归。如果 055 改变了行内范围几何 API，则由 055 同步适配 031 的消费者。

## 实施与验收

- [x] 010 提供当前块结构的稳定 ID 和同代分页范围；Rust 测试覆盖跨页范围分片。
- [x] Qt ViewModel 接收并清空块高亮快照；替换或清空后旧矩形不会残留。完整快照的旧 generation 拒绝行为尚未由此项验证。
- [x] Qt scene graph 使用独立圆角块高亮层，按页面几何裁剪并置于选区和字形下方；颜色、圆角和外扩留白来自 GUI Theme。
- [x] 默认与深色主题定义块环境颜色；命令提示使用独立 token。高亮不进入 `.cdoc`。
- [x] Rust 从光标位置解析最内层行内环境（以 `Bold` 验收），将稳定环境 ID、段落 ID 和 UTF-8 范围映射到同代布局几何。
- [x] 行内背景按真实文本范围/视觉行切片，覆盖软换行和长环境，不扩展成全段高亮；空范围不产生可见背景。布局几何沿用 Parley 选区路径，以支持 bidi 排列。
- [x] 行内/块级环境切换遵守既有 affinity；行内矩形非空时优先显示，离开后恢复块环境几何；插入点位置更新和空快照会替换或清空当前矩形。
- [x] 行内、块级、命令提示三种颜色在所有主题均独立；行内默认浅色为 `#FCE4EC`，深色为低对比度粉色。正文环境由同一个通用 QSG 环境高亮层绘制，命令提示保留 QML 控件背景；文字、选区、光标层次正确，文档存储和 generation 不变。
- [x] 行内与块级行为测试及独立评审通过；未执行的真实窗口走查明确记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-30 | `cargo test --locked --offline -p caret-app block_environment_geometry_is_emitted_as_page_fragments` | 跨页块环境范围被正确拆为每页矩形 | 通过；实际跨页 Section 产生有效页内片段，见 010 验证记录 |
| 2026-10-30 | Qt Quick `ShellLoadTest::documentCanvasMapsPageCoordinatesAndHitsPages`、`ShellLoadTest::environmentHighlightSnapshotSelectsAndClearsActiveRects`（native CTest 聚合） | 页面裁剪、范围恢复与清空行为正确 | 通过；场景图逐页截帧检查通过，CTest 8/8，见 010 验证记录 |
| 2026-10-30 | 代码与资源交叉检查 | QML/Theme、Rust snapshot、ViewModel、QSG 图层和测试入口一致 | 通过；块高亮使用 `color-block-environment-highlight`，命令提示使用独立 token；未发现文档序列化路径 |
| 2026-10-30 | `cargo test --locked --offline -p caret-engine -p caret-app` | 最内层环境、affinity、跨行范围和文档视图投影行为正确 | 通过；caret-engine 143 项、caret-app 181 项通过，包含嵌套 Bold/Italic、边界亲和性、软换行、多行几何、Latin/Arabic/Hani 混排范围、IME 预编辑范围同步及空行内环境回退 |
| 2026-10-30 | `cargo test --locked --offline -p caret-tests --test native` | 通用 QSG 环境层可绘制分页背景；ViewModel 在新快照中优先选行内范围、行内范围清空时回退块级，并在两者清空后不残留 | 通过；CTest 8/8，包含 Shell ViewModel 快照行为和场景图测试 |
| 2026-10-30 | `ShellLoadTest::environmentHighlightSnapshotSelectsAndClearsActiveRects` | 迟到旧 generation 即使携带损坏字段也不清除当前高亮；当前 generation 的无效交互状态清除高亮并通知绑定 | 通过；同时断言页面投影不变、过期结果不发交互通知 |
| 2026-10-30 | `cargo test --locked --offline -p caret-doc` | 已知行内环境使用集中注册表解析；未知 kind 仍不透明往返 | 通过，30 项 |
| 2026-10-30 | `cargo fmt --all --check`、`cargo clippy --locked --offline -p caret-doc -p caret-engine -p caret-app --all-targets -- -D warnings`、`cargo test --locked --offline -p caret-doc -p caret-engine -p caret-app` | Rust 格式、告警和受影响 crate 行为符合仓库规则 | 通过；clippy 无告警，相关 crate 测试通过 |
| 2026-10-30 | `cargo coverage` | Rust 全局函数与行覆盖率均不低于 85% | 通过；函数 85.86%，行 89.51% |
| 2026-10-30 | `cargo coverage native` | native 产品源码函数与行覆盖率均不低于 85%，并运行 QML/native CTest | 通过；CTest 8/8，函数 89.78%，行 85.03% |
| 2026-10-30 | 真实窗口人工走查 | 确认块/行内高亮切换、失效清除和视觉层级 | 本轮未执行；已有用户手工体验反馈，但不把它记作本轮验收证据 |
| — | 031/055 集成回归：Bold 环境内混排 Latin/Hani，粗体 face 与 BASE 对齐 | 高亮始终覆盖 Bold 的塑形范围并沿用共享行框；基线对齐不改变高亮/行框高度 | 055 实现后执行；不作为 031 独立实现的前置条件 |

## 风险与回退

行内环境可能跨多视觉行、软换行或 bidi run；必须消费布局层选区几何，不能用简单 x 坐标和字节顺序拼矩形。当前快照或当前 generation 的交互几何失效时清除高亮并保留正文；与当前 generation 不一致的迟到交互结果直接丢弃，不得覆盖或清除较新状态。可恢复的旧局部布局快照保留当前投影并请求完整快照。当前每个视觉行矩形会被圆角切片展开为多个 QSG 子节点；节点总数随可见高亮行数线性增加。031 未有实测证据证明这已构成瓶颈，代表性长环境测量和必要的批量几何优化归入 041 性能任务，不能把节点减少本身视为已验证的性能收益。

## 决策与工作记录

- 2026-10-26：从 027 拆分环境高亮。
- 2026-10-29：明确命令提示、行内环境和块环境分别使用 GUI 背景语义；命令提示由 009 接入，010 提供块结构范围。
- 2026-10-29：块高亮使用单独 QSG 场景层，复用分页布局几何；采用 Theme 颜色与圆角，维持纸张→环境高亮→选区→字形→光标的层级。
- 2026-10-30：实现前确认 010 已完成块结构和几何基础，但行内 `Bold` 等当前环境仍没有画布高亮；当时暂留 031 为 `in-progress`，并将 `Bold` 作为实现和行为测试样例。
- 2026-10-30：确定嵌套显示规则：最内层环境独占当前高亮；行内环境进入时暂时替代祖先块环境高亮，退出后恢复块环境高亮。
- 2026-10-30：确定命令提示与文稿环境背景的渲染边界，详见[画布与交互背景](../architecture/interface/canvas-highlighting.md)。块与行内背景共用 QSG 绘制层；命令提示作为有焦点和内容布局的 QML 弹出控件保留在 QML 层。
- 2026-10-30：接入最内层行内环境的 Rust 范围投影、同代布局矩形、ViewModel 快照与通用 QSG 绘制层；通过跨行几何及 Qt Quick 画布/快照替换测试。
- 2026-10-30：评审发现 IME 预编辑快照必须从同一个预览 Session 解析环境范围；空行内环境不应隐藏可见块高亮。已按此修复并添加行为测试。
- 2026-10-30：独立评审发现快照失败后的高亮失效规则需区分当前数据损坏与迟到旧 generation；已修复 ViewModel 失效策略、明确文档，并增加可观察投影与通知断言。行内 kind 解析入口改为唯一注册表，未知 kind 往返测试通过。
- 2026-10-30：Terra 分领域评审和 Sol 完整 diff final review 均无阻断 findings；Rust 30 项和 native CTest 8/8 通过。初次评审时未执行真实窗口人工复测或 Rust/native 覆盖率核验；后续已运行两项 Nightly 覆盖率门槛并通过。041 后续承担性能基线与测量，055 承担 BASE 混排回归。

## 完成摘要

块与行内环境当前项高亮已接入：Rust 负责当前环境语义与布局范围，ViewModel 只投影选中几何，画布使用单一 QSG 环境高亮层；命令提示由独立 QML 控件绘制。专项和最终评审无阻断问题，任务状态为 `done`。Rust/native 覆盖率门槛已通过；本轮未执行真实窗口人工复测，作为明确限制保留。041 后续承担性能基线与测量，055 承担 BASE 混排回归。
