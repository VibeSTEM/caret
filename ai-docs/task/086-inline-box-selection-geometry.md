# 086 — 行内对象边界选区几何修正

- 状态：planned
- 依赖：030、044、068
- 优先级：P1
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

`caret-layout` 通过 Parley 计算文本选区矩形时，Parley 0.12 会把行内 box 的 advance 累加到“box 之后第一个字素簇”的 advance 上（`parley-0.12.0/src/editing/selection.rs:567-600`）。`caret-layout` 把行内对象 box 插在 `index = object.source_bytes.end`（`crates/caret-layout/src/paragraph/parley.rs:1083-1094`，U+FFFC 用等长 U+200B 替换，`:1006-1011`），刚好触发该边界：当选区起点落在对象之后第一个字素簇时，选区矩形 x0 会从对象左缘之前开始，覆盖相邻行内对象。

用户可观察症状（2026-11-12 桌面测试）：

- InlineMath 与 `\bold` 等 InlineEnvironment 紧贴时，光标在 `\bold` 内会让紧邻的 InlineMath 出现粉色内联环境高亮（实为粉色高亮矩形从对象左缘开始）。
- 选中 InlineMath 之后的正文时，蓝色选区矩形向左多覆盖 InlineMath 对象盒。
- 下划线等 RunProperty 插入槽同样覆盖相邻 InlineMath。

问题在 `main` 已存在，不是跨宿主选区 PR 引入；由 [math 布局](044-math-layout-and-rendering.md)引入的行内对象映射触发。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[数学公式](../architecture/editor/math.md)
- `crates/caret-layout/src/paragraph/parley/interaction.rs`、`crates/caret-layout/src/paragraph/parley.rs`
- [044](044-math-layout-and-rendering.md)、[069](069-math-interaction-and-platform-acceptance.md)

## 范围与非目标

- 修正 `ParleyParagraphLayout::selection_geometry` 的 inline object 边界：对未被选区完整覆盖的行内对象，按 `positioned_inline_box` 的 `[x, x+w]` 裁掉与之重叠的选区矩形（左右可拆成两段）后排序。
- 覆盖 InlineMath、InlineEnvironment 与普通 U+FFFC；保持选区完整包含对象时仍输出对象整体矩形。
- 非目标：不改变文档位置、选区事务、命中、Qt 绘制；不处理空 InlineMath 宽度（由 [087](087-empty-inline-math-advance-and-highlight.md) 跟踪）。

## 前置条件与待决策

- 已确认根因与原型（2026-11-12 只读调查）：修正后粉色高亮在 math-then-bold 场景 x0 从 72 变为 78、带前缀从 78.59 变为 84.59；蓝选区 x0 同步；math 位于 Bold 内时保持整框。
- 需确认 RTL、跨行、同簇多对象邻接场景的裁切规则；选区矩形由 Parley 返回 `Vec<PageSelectionRect>`（页面坐标）。

## 实施步骤

1. 在 `selection_geometry` 的对象矩形补充循环（`interaction.rs:222-246`）后，对每个未被选区完整覆盖的 inline object 用 `positioned_inline_box`（`:385-396`）取盒区间，裁掉重叠选区矩形。
2. 补行为测试：选区起点在对象之后时 x0 == 对象右缘；选区完整覆盖对象时保留整框；RTL 版本；多对象邻接。
3. 跑 `cargo test -p caret-layout`、`-p caret-app`，并做 app 层粗/斜体与 InlineMath 邻接的高亮回归。
4. 按验证标准做分域与独立 final review。

## 预计改动

`crates/caret-layout/src/paragraph/parley/interaction.rs`、`crates/caret-layout/src/paragraph/interaction/tests.rs`、必要的 app `document_view` 行为测试。

## 验收标准

- [ ] 选区/高亮矩形在对象之后开始时不再覆盖对象盒；对象被完整选中时仍覆盖。
- [ ] RTL、跨行、多对象邻接与空对象行为有回归；旧行为断言同步更新。
- [ ] 格式、lint、相关 crate 测试与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-layout`、`-p caret-app` | 几何与高亮回归通过 | 未执行 |
| 2026-11-12 | 只读调查（原型 ~30 行 + 现有测试） | 根因与修复可行 | 已确认，临时原型未提交 |

## 风险与回退

影响所有文本选区几何，需 RTL/跨行/多对象回归；当前公式不跨页，新 API 未来跨页需按 projection 逐页返回。若裁切规则无法统一，保留对象整体矩形并记录为已知视觉偏差，回退单点修改。

## 决策与工作记录

- 2026-11-12：从 PR #136 桌面测试发现中拆出；只读调查确认 main 已存在，归 044/030 几何范围；建议独立 fix PR，不混入 #136。
