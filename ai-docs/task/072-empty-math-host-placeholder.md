# 072 — 空公式宿主占位的视觉与几何契约

- 状态：done
- 依赖：044
- 优先级：P2
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

空 Equation、Align 空单元格/行及空 InlineMath 当前可能复用数学结构内部必填槽的描边占位框。数学根/宿主的占位应保留足以命中、编辑和放置光标的非零节点几何，但不应与 `\frac{}{}` 等公式内部必填槽显示成相同的矩形。本任务落实 [issue #106](https://github.com/VibeSTEM/caret/issues/106)；需要在当前 HEAD 重新确认根流、显式 Placeholder 与页面快照路径，再决定最窄的表示层。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[数学公式架构](../architecture/editor/math.md)、[044 公式布局与画布](044-math-layout-and-rendering.md)、[069 数学交互验收](069-math-interaction-and-platform-acceptance.md)

## 范围与非目标

- 区分数学结构内部必填槽与公式宿主/根流空态的布局角色。
- 宿主根占位保留非零节点盒、点击区域、插入位置与 caret 几何；仅移除不合适的矩形绘制输出。
- 内部必填空槽（分数、上下标、根式等）继续绘制现有占位框；可选槽仍保持不绘制。
- 覆盖空 Equation、空 Align 及空 InlineMath 根流，确保宿主空态不再呈现为公式内部必填槽。
- 空根可进入编辑后，Qt 快照校验应接受 Rust 发布的数学 `math` 子命令模式，确保反斜杠命令输入和导航快照能够继续往返。
- 不按交互焦点决定数学布局输出，不使缓存 key 依赖会话焦点；不改 MathTree 持久化语义，不移除用于插入点映射的几何。
- 若新的布局角色影响缓存输出，缓存键需能区分该稳定布局输入；不要把焦点等瞬时交互状态带入缓存。

## 前置条件与待决策

- 044 已完成；实施前确认各公式类型如何生成根流/占位节点以及 engine 对节点盒的命中和 caret 要求。
- 选择 `Item::Rect`/节点盒角色或根流布局上下文等最小表达方式；不能只根据当前节点形状推断“宿主”与“必填槽”。
- 空宿主最小高度与普通段落行高的关系尚未定案，须结合不同字号、缩放、块背景、焦点协议及 inline baseline 验证；不可将块级行高直接应用到行内公式并改变周围行距。

## 实施步骤

1. 为 `layout_math`、`layout_align` 和页面投影建立空内容、单 Placeholder 宿主根、含空 cell 的非空 Align 快照回归，确认现在输出何种图元和节点盒。
2. 定义根占位与内部槽的稳定布局语义，保留尺寸并避免产生根占位矩形。
3. 验证空公式鼠标进入、键盘焦点、caret 几何、编辑、删除和撤销；确认内部必填槽视觉不变。
4. 更新数学架构和 069 的空 cell 交互边界，执行分域审查、独立 final review 并复验。

## 预计改动

`crates/caret-math/src/layout/`、`crates/caret-engine/src/page_glyphs.rs` 的投影/测试及数学布局行为测试；必要时更新 [数学公式架构](../architecture/editor/math.md)、[044](044-math-layout-and-rendering.md) 与 [069](069-math-interaction-and-platform-acceptance.md)。不需要新增 crate 或 FFI 字段，除非实现证据显示已有快照契约无法表达。

## 验收标准

- [x] `layout_math` 对空 MathFlow 的 Display/Inline 样式、空 Align 内容和单 Placeholder 宿主 cell、空 InlineMath 根不输出宿主占位描边矩形，且根节点盒宽高大于零；非空结构中的合法矩形不受影响。
- [x] 空宿主可点击进入并产生稳定的插入点几何；页面命中保持有效。app 路径覆盖空 Equation 点击、输入、撤销和重做；空宿主整块删除与历史恢复沿用既有数学会话回归。
- [x] Qt 快照校验接受公式 `math` 子命令状态；集成测试进入 InlineMath、打开 `\` 查询、输入 `alpha` 并通过视觉导航接口，不再返回 `layout.invalid_command_line`。
- [x] `\frac{}{}`、空上下标、根式必填槽等内部占位矩形保持现有呈现；可选槽行为不变。
- [x] 视觉结果不依赖焦点状态，布局缓存仍只以公式内容、字号、样式和字体为键。
- [x] 快照、保存恢复、交互测试、格式/lint 和独立评审通过；最小高度、inline baseline 及平台截图验证如未覆盖须明确记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-math layout::tests::` | 根/空 cell 无框但有非零节点盒，必填槽仍有框 | 53 passed；包括 Display/Inline、Align、单 Placeholder 根、shaper 路径和内部槽位回归 |
| 2026-11-08 | `cargo test --locked -p caret-engine`；`cargo test --locked -p caret-app` | 页面命中、光标、编辑与历史路径无回归 | engine 654 passed；app 256 passed。新增 app 测试覆盖空 Equation 点击、caret、输入、Undo/Redo |
| 2026-11-08 | `cargo test --locked -p caret-tests --test native` | Qt 安装 `math` 命令状态并继续处理公式输入/导航 | CTest 10/10 passed；shell 回归使用真实 `insertText("\\")`、`insertText("alpha")` 与左移，命令光标 5→4 |
| 2026-11-08 | `cargo lint --check` | 跨语言 clippy、依赖、CMake、QML/native lint 通过 | 9/9 stages passed；包含 native build、qmllint、clang-tidy、include-cleaner 和 cppcheck |
| 2026-11-08 | `cargo format --check`；`git diff --check HEAD` | 全仓格式和差异空白检查通过 | 两项均通过 |

## 风险与回退

占位框可能同时服务视觉提示和 MathTree 节点盒映射。只隐藏矩形，不把节点宽高改为零或修改节点身份，避免破坏 caret/命中。若新角色无法由适配器稳定区分，保留现有图元直到有完整表示与回归覆盖。

## 决策与工作记录

- 2026-11-08：通过 `gh issue view 106` 审查问题；本地当前布局仍对空 flow Row 与 Placeholder 调用同一 `placeholder()`。分域复核确认 Align 宿主还存在单 Placeholder 形态；engine 对零尺寸节点会跳过命中槽。采用视觉图元与节点盒分离的验收方向，不把建议的普通行高对齐当成既定产品行为。
- 2026-11-08：从合并 PR #115 后同步的 `main` 创建 `yuki/task-072-empty-math-host-placeholder`；继续确认数学布局转换输出与 engine 命中、页面绘制之间的职责边界。
- 2026-11-08：`caret-math` 对仅根流（空根或单 Placeholder 根）布局后隐藏 Rectangle 图元，保留节点记录和非零盒；嵌套必填操作数仍经过原有可见占位布局。引擎回归覆盖空 InlineMath、Equation 与 Align cell 的页面形状、根盒和结构命中；app 回归覆盖空 Equation 点击、树焦点、输入、Undo/Redo。
- 2026-11-08：真实 QML 日志追查发现 native 快照校验遗漏 Rust `CommandLineMode::Math`，导致 `\` 查询和导航快照被拒为 `layout.invalid_command_line`；已扩展合法 mode/prefix 并补 Qt 集成回归。光标仲裁仍统一走架构文档规定的 EditorKey 入口；公式专用部分只构造布局候选。
- 2026-11-08：MathText 聚焦期间活动 math command line 的 Left/Right/Up/Down 先直接提交给统一 EditorKey 入口；补 app 回归验证普通 MathText 工作后可开启命令并移动查询光标。
- 2026-11-08：Terra 分域审查及 Sol 完整最终 diff 只读 review 完成，无阻断发现；将 074 已实现的命令行优先派发从待实现步骤改为回归维护项。final review 已复核该更正。桌面截图和 fraction 上下移动留在后续任务边界。
- 2026-11-08：本次未运行 macOS/Windows/Linux 桌面截图检查；实现不改变宿主盒尺寸和 baseline，跨平台视觉检查留作明确的待验边界。既有 app suite 覆盖文档会话保存恢复，数学会话 suite 覆盖空宿主删除与撤销恢复。
- 2026-11-08：PR [#116](https://github.com/VibeSTEM/caret/pull/116) 的必需 CI 全部通过并已 squash 合并，merge commit `11dc1d4e`；issue #106 已关闭。

## 完成摘要

代码与文档实现完成，直接消费者及跨语言验证通过，独立审查无阻断；PR #116 已合并，issue #106 已关闭。
