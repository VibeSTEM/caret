# 086 — 行内对象边界选区几何修正

- 状态：implemented
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
| 2026-11-12 | 只读调查（原型 ~30 行 + 现有测试） | 根因与修复可行 | 已确认 |
| 2026-11-12 | `cargo test --locked -p caret-layout` | LTR/bidi/完整覆盖/邻接对象回归 | 通过：133 passed / 0 failed（新增 3 条） |
| 2026-11-12 | `cargo test --locked -p caret-app` | 粉色高亮/普通选区不覆盖相邻 InlineMath | 通过：313 passed / 0 failed（新增 2 条） |
| 2026-11-12 | `cargo test --workspace --locked`、workspace clippy `-D warnings`、fmt check | 全量无回归 | 通过（exit 0） |
| — | Qt 桌面截图/窗口验证 | 粉色与选区绘制观感 | 未执行（本环境无窗口） |

## 风险与回退

影响所有文本选区几何，需 RTL/跨行/多对象回归；当前公式不跨页，新 API 未来跨页需按 projection 逐页返回。若裁切规则无法统一，保留对象整体矩形并记录为已知视觉偏差，回退单点修改。

## 决策与工作记录

- 2026-11-12：从 PR #136 桌面测试发现中拆出；只读调查确认 main 已存在，归 044/030 几何范围；建议独立 fix PR，不混入 #136。
- 2026-11-12：按用户继续指令在 PR #136 分支追加实现（与 087 分开提交）：`selection_geometry` 在完整覆盖补框之后，对未被完整覆盖的 inline object 用 `positioned_inline_box` 取盒区间，把重叠选区矩形裁除（可拆左右两段）后保持原排序；完整覆盖仍保留整框。几何证据：LTR `a\u{fffc}b` 选区 4..5 的 rect.x0 从对象左缘 6.023 修正为对象右缘 36.023；bidi 场景 18.68→48.68；app 粉色/蓝色矩形 x0 从 72.0pt（math 右缘 78.0）变为 ≥78.0。反向验证：临时还原修复后 5 条新测试全部失败。遗留：每选区对每个 inline object 做一次区间裁剪（O(objects×rects)），对象数通常很少，大规模长选区纳入 041 基准考虑；增量评审建议补“对象在第 1 行、选区跨到第 2 行”的用例与 087 后零宽对象（advance=0）裁切回归，当前已由 087 的 engine 空公式选区回归覆盖零宽场景，跨行用例留待 069 平台验收补充；Qt 截图与 zoom 矩阵未验证。

- 2026-11-12：增量独立复核结论 mergeable（最终树含 087 零宽修复）：`selection_geometry` 裁切只影响选区几何、完整覆盖保留整框、RTL/邻接/完整覆盖通过；087 的零宽对象在补框循环被跳过，engine 投影另有 x1<=x0 防御过滤。遗留：跨行选区用例留 069 平台验收；每选区 O(objects×rects) 纳入 041 基准考虑；Qt 桌面未验证。
