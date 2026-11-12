# 087 — 空 InlineMath 行内推进与聚焦高亮契约

- 状态：implemented
- 依赖：044、072、074
- 优先级：P1
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

[072](072-empty-math-host-placeholder.md) 隐藏了空公式宿主的描边占位框，但保留了 0.6em×0.7em 的根节点盒。空 InlineMath 的该盒宽度被 engine 当成正文行内 advance，导致对象两侧出现额外间隙；聚焦时 app 又按 U+FFFC 的真实文本跨度绘制内联环境高亮，得到 7.2pt 左对齐粉色矩形，与空 `\bold` 等 InlineEnvironment 的 6pt 居中插入槽不一致。

用户可观察症状（2026-11-12 桌面测试，12pt 默认字号）：

- 空 InlineMath：`MathLayout.width=7.2pt`，段落中 `A\u{FFFC}B` 的对象 advance 为 7.2pt，行右边界比纯文本多 7.2pt 空白。
- 空 Bold InlineEnvironment：advance=0；聚焦插入槽 6pt 居中。
- 聚焦空 InlineMath：粉色矩形 x0=79.623、x1=86.823（宽 7.2，caret 在左缘），而空 Bold 槽为 caret 两侧各 3pt。
- 非空 InlineMath 的 advance `MathLayout.width` 与内容高亮正确，不受影响。

## 必读

- [数学公式](../architecture/editor/math.md)、[072](072-empty-math-host-placeholder.md)、[044](044-math-layout-and-rendering.md)、[068](068-shared-grid-layout-and-math-page-architecture.md)、[069](069-math-interaction-and-platform-acceptance.md)
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)
- `crates/caret-math/src/layout.rs`、`crates/caret-math/src/layout/convert.rs`、`crates/caret-engine/src/layout.rs`（`InlineMathLayoutBuilder`、`hit_test_math_structure`）、`crates/caret-app/src/document_view/snapshot.rs`

## 范围与非目标

- 新增空宿主行内推进契约：空 InlineMath 的 MathNodeBox 保留非零几何（072 的命中/caret 锚点验收不变），但正文行内 advance 为 0，与空 InlineEnvironment 一致；聚焦粉色高亮改用与其他空 inline 一致的插入槽（宽 `max(0.5em, 4pt)`、以 math caret x 居中、y 沿用当前行高）。
- 同步命中：空公式仍可点击进入，但命中区限制在插入槽范围，不得吞掉右侧相邻文本点击（app 命中顺序 math_structure 先于 body）。
- 非空 InlineMath、Equation/Align 块布局与分页不变。
- 非目标：不改变 072 的节点盒/无描边验收；不虚构零尺寸导致 caret/命中失效；不处理行内对象选区几何 bug（由 [086](086-inline-box-selection-geometry.md) 跟踪）。

## 前置条件与待决策

- 已确认根因链（2026-11-12 只读调查）：caret-math 空宿主隐藏帧保宽 → engine 把 `layout.width` 当 advance（`layout.rs:4443-4456`，1.5pt 零推进回落在无 sidecar 时不触发）→ app 按 U+FFFC 文本跨度画高亮（`snapshot.rs:393-443`；空 InlineEnvironment 走另零长度 span 插入槽）。
- 待决策：“零推进（advance=0）”与“极小可点击推进（1.5pt）”二选一；推荐 0（与空 `\bold` 完全一致，命中槽兜底）。对齐、缩放和字号矩阵（0.5em/4pt 下限）需在实现时验证。

## 实施步骤

1. `caret-math`：为 `MathLayout` 增加显式 `inline_advance`（Inline 且空宿主 → 0.0，其余 `width`）；不改 frame/节点盒宽高。
2. `engine`：`InlineMathLayoutBuilder` advance 改用 `inline_advance`；空宿主命中改为插入槽范围；`EnvironmentEntry` 增加 `empty_inline_host` 标记供 app 投影。
3. `app`：`inline_environment_highlight_rects` 对空 InlineMath 使用与 `insertion_slot_rect` 相同的槽规则；其余不变。
4. 补 caret-math/engine/app 回归：advance、caret x、命中不吞邻字、粉色槽与空 `\bold`/ToggleInlineFormat 同宽同居中、非空公式不变、空公式点击进入、删除/Undo/IME 不回归；字号/zoom 矩阵。
5. 按验证标准做分域与独立 final review；Qt 实机截图核对颜色/padding/radius。

## 预计改动

`crates/caret-math/src/layout.rs`、`crates/caret-math/src/layout/convert.rs`、`crates/caret-math/src/layout/tests.rs`、`crates/caret-engine/src/layout.rs`、`crates/caret-engine/src/layout/tests.rs`、`crates/caret-engine/src/editing/environment.rs`、`crates/caret-app/src/document_view/snapshot.rs` 及 app 测试、`ai-docs/architecture/editor/math.md`（空宿主推进契约）。

## 验收标准

- [ ] 空 InlineMath 在正文中 advance 为 0；相邻字符位置与空 InlineEnvironment 一致，无非可见墨迹的额外间隙。
- [ ] 聚焦空 InlineMath 的粉色高亮与其他空 inline 插入槽同宽、同居中规则；非空公式与块公式几何不变。
- [ ] 空公式仍可点击进入并编辑，命中槽不吞右侧文本；caret、删除、Undo/Redo、IME 回归通过。
- [ ] 0.9/12/20pt 字号与 zoom≠1 矩阵、Qt 截图核验；格式/lint/测试/独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-12 | 只读调查（临时 engine/app 探针） | 定位 advance/高亮来源并验证可行 | 已确认（数值见目标与背景） |
| 2026-11-12 | `cargo test --locked -p caret-math -p caret-engine -p caret-app` | 行内推进/高亮/命中/删除/Undo 回归 | 通过：math 136、engine 843、app 314，0 failed；空公式 IME 未单独覆盖（既有非空数学 IME 用例通过） |
| 2026-11-12 | `cargo test --workspace --locked`、workspace clippy `-D warnings`、fmt check | 全量无回归 | 通过（exit 0） |
| — | Qt 桌面截图（颜色/padding/radius/zoom≠1） | 空公式粉色槽与空 Bold 视觉一致 | 未执行（本环境无窗口） |

## 风险与回退

- 零推进可能影响空公式点击命中；必须同批改命中槽并验证不吞邻字，否则回退到 1.5pt 极小推进。
- 若 `EnvironmentEntry` 标记影响其他环境视图消费者，优先在 app 侧用现有 math caret 几何判断空宿主，避免扩散状态。
- 空公式删除/IME 路径依赖 U+FFFC offset 语义，不改文本跨度；若回归则回退 advance 层单独修复。

## 决策与工作记录

- 2026-11-12：从 PR #136 桌面测试发现中拆出；只读调查确认 072 契约漏了“隐藏盒宽进入正文 advance”和“粉色按 U+FFFC 范围而非插入槽”两条；建议独立 fix(072/044) PR，不混入 #136。
- 2026-11-12：按用户继续指令在 PR #136 分支追加实现（与 086 分开提交），决策采用“零推进 + 插入槽命中”而非 1.5pt 极小推进：caret-math `MathLayout.inline_advance`（只在 Inline 空宿主为 0，frame/节点盒 0.6em×0.7em 与 072 描边契约不变）；engine advance 改用该字段，空宿主命中以公式 caret 居中的插入槽（`max(0.5em,4pt)`）并在点落入正宽正文/文本岛字素内部时跳过公式候选，避免吞邻字；`EnvironmentEntry.empty_inline_host` 单一标记 + app 复用插入槽投影。数值：12pt 空 InlineMath advance 0、相邻文本 caret x 与空 Bold 一致（79.623）、粉色槽 6pt 居中、9/20pt 为 4.5/10pt；命中槽中心进入公式、邻字内部落正文；空公式 Backspace→"AB"、Undo 恢复。072 既有命中采样点按新契约调整（Equation/Align 不变）。遗留：零推进后实际可点入区域约为 caret 两侧各 1.5pt（视觉槽更宽但边缘按字素内缩归正文），桌面验收需按此预期检查；engine 的字素内缩判定与 app 命中语义目前两处实现，建议后续下沉为单一 helper；Qt 截图/zoom 未验证。
- 2026-11-12：增量独立评审发现并修复阻断：选区恰好覆盖空 InlineMath 时，caret-layout 的“完整覆盖对象补整框”会补出零宽矩形（advance=0），Qt 快照 codec 因 x1<=x0 拒绝整份快照。修复：caret-layout 补框跳过 width<=0 对象；engine selection/composition 投影防御性过滤 x1<=x0；app 空宿主在无公式 caret（Source 分支）时退回共享 caret/行框插入槽而不是按 U+FFFC 范围投影。补 engine/app 回归：选中空公式 `1..4`、段落首 `0..3` 不产生零宽 selectionRects。

- 2026-11-12：B1 修复后的短增量独立复核结论 mergeable：caret-layout 补框跳过零宽对象、engine 投影过滤零宽、app Source 分支退回插入槽三条均落地；独立 probe 复测 `A{math}B` 选区 1..4、段落首 0..3 现在返回空矩形而非零宽，相邻文字正常选区不变；caret-math/engine/app 全绿，Qt codec 的 x1<=x0 拒绝分支不可再达。残留：engine/app 的插入槽与正文优先规则仍两份实现，建议后续下沉为单一 helper；Qt 桌面截图与 zoom≠1 未验证。
