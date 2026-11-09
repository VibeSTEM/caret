# 075 — Equation / Align 活动块背景高亮

- 状态：done
- 依赖：044、072
- 优先级：P2
- 创建 / 更新：2026-11-08 / 2026-11-09

## 目标与背景

编辑 Equation 或 Align 时，当前公式块没有奶黄色背景。当前快照只为活动块环境投影 `environment_ranges`，而 QML 画布只消费 Rust/native 发布的矩形；显示公式虽有分页 MathFragment 几何，却未加入活动块高亮投影。

为活动 Equation 与 Align 提供稳定的块级背景几何，复用现有块环境高亮色、圆角与 padding，并确保分页、空公式与 Align cell 聚焦时反馈一致。键盘状态与方向键仲裁仍按[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)统一由 `Session::run_editor_key` 管理；本任务不新增数学专用光标状态或按键路由。

块级奶黄色背景之外，Equation/Align 内的 `\text{...}` 文本岛在编辑时还需要现有粉色 inline 高亮，提示当前 IME 文本输入位置；两种高亮应能同时正确显示，文本岛高亮只覆盖活动文本范围，不替代公式块高亮。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[数学公式架构](../architecture/editor/math.md)、[画布高亮唯一权威](../architecture/interface/canvas-highlighting.md)、[当前环境背景高亮](031-environment-highlight.md)、[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)、[072 空公式宿主占位](072-empty-math-host-placeholder.md)

## 范围与非目标

- 活动 MathEdit target 为块级 Equation 或 Align 时，为其可见页面片段发布高亮矩形；InlineMath 不显示块背景。
- 活动 Equation/Align TextFlow（包括 `\text{...}`）保留粉色 inline 输入高亮；块级奶黄色底色与文本岛高亮按独立几何绘制并可共存。
- Align 按整个活动 Align 宿主高亮；cell 不单独着色，活动 `\text` island 的粉色高亮表达更细的文本焦点。
- 活动 MathEdit Block 不从公式外停靠的正文位置推导块环境高亮，以免误亮相邻 Section。
- 数学块背景覆盖页面版心内容宽度，纵向使用活动 MathFragment 的页面片段高度并裁切到页面；MathFragment 宽度仍表示公式本身，不用它限制块背景宽度。
- 当前分页器不拆分单个数学块；验证公式在多页文档中的所在页、页面可见性和页面裁切，不声称单个公式跨页切片。
- 无有效活动 MathEdit Block、结构选区或公式退出停靠点时不发布公式块高亮；旧 generation 的交互快照不得覆盖当前高亮。交互安装目前同步，不声称处理同 generation 异步乱序。
- 复用现有主题颜色，不引入新颜色设置；不把 Equation/Align 伪装成文档块环境模型。

## 前置条件与待决策

- 072 的空宿主节点盒与命中契约完成并合入。
- 版心全宽背景沿用现有块环境视觉语义；MathFragment 仅提供公式块准确的纵向位置与高度。

## 实施步骤

1. 追踪经 revision 校验的活动 MathEdit target、MathFragment、MathText island 和现有高亮的快照/codec/canvas 链。
2. 在权威快照层分别投影活动公式块黄底和活动数学 TextFlow 粉底；不把公式伪装成文档环境，也不在 Qt 重建数学文档状态。
3. 由 Qt ViewModel 校验独立来源并将 Equation/Align 与普通块环境几何合并为同一块高亮属性；Section 和公式块复用同一黄色 Canvas 绘制层，粉色 inline 层单独叠加。
4. 覆盖 Equation/Align、空/非空、MathText、普通正文、父块环境、公式进出和旧快照回归；在画布高亮唯一权威中记录契约，数学文档只链接该说明，并完成独立评审。

## 预计改动

`crates/caret-app/src/document_view/snapshot.rs`、`native/viewmodels/document/layout_snapshot_codec.cpp` 与 ViewModel、`native/canvas/`、`qml/PageSurface.qml`，以及 Rust/C++/QML 测试。具体接口和模块边界按现有快照传输路径实现。

## 验收标准

- [x] 活动 Equation 与 Align 显示奶黄色全内容宽度背景，纵向矩形与整个当前 MathFragment 宿主一致；Align cell 不单独着色；InlineMath 和非活动公式不出现块背景。
- [x] Equation/Align 内活动 `\text{...}` 文本岛显示粉色 inline 高亮，范围覆盖完整活动 TextFlow island、跟随 IME 预览布局，并与块级奶黄色背景共存；输入法 underline 保持独立绘制。
- [x] 空宿主仍有高亮几何且不依赖 placeholder Rectangle；Align 空 cell 的选定粒度稳定。
- [x] 多页文档中的公式仅在 MathFragment 所在页面显示并遵循页面裁切；点击切换焦点、键盘离开、undo/redo 和结构选区不会保留旧高亮或误高亮停靠位置的邻接块环境。
- [x] 超出纸张 x/y 边界的数学文本岛高亮先按所在页裁切，再进入 Qt 校验；不因长 Equation/Align 文本而拒绝整份交互快照。另记录：已有超宽 x 越界行为测试，尚无专门 y 越界 fixture。
- [x] 快照分别保留数学块与文档块的领域来源；Qt ViewModel 合并为共用块高亮属性，Section 与 Equation/Align 使用同一 Canvas 绘制实现；粉色 inline 层可同时显示。
- [x] 非法坐标/旧 generation 快照被拒绝且不替换当前已发布状态；主题绑定及通知贯通。
- [x] Rust/native/QML 行为测试、格式/lint 与独立评审通过；未执行的真实窗口走查已记录，Qt Quick 像素测试验证黄色块层和粉色 inline 层叠加。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-09 | `cargo test -p caret-app --locked` | 活动 Equation/Align、空 Equation、MathText IME 预览、超宽文本裁切和 Section 停靠状态正确 | 通过，275 项；超宽文本有 x 越界用例，y 越界目前由实现裁切但尚无专门溢出 fixture |
| 2026-11-09 | `cargo test -p caret-engine --locked` | MathText island 端点与经 revision 校验的 MathEdit target 正确 | 通过，677 项 |
| 2026-11-09 | `cargo test -p caret-tests --test native --locked` | Qt ViewModel 来源合并、非法快照原子拒绝、QSG 层顺序/颜色与 QML 绑定正确 | 通过，CTest 10/10 |
| 2026-11-09 | `cargo clippy --locked -p caret-engine -p caret-app --all-targets -- -D warnings`、`cargo fmt --all --check`、修改文档本地链接检查、`git diff --check HEAD` | Rust 质量门禁与文档引用通过 | 通过；文档链接检查 7 个文件 |
| 2026-11-09 | Terra Rust/UI 分领域审查 | 几何裁切、焦点/IME 状态、来源复用和跨层边界无阻断问题 | 通过；按审查修正 IME underline 几何、页面边界、颜色采样、文档链接及来源说明 |
| 2026-11-09 | Sol 独立 final review（完整 `git diff HEAD`） | 跨层状态、复用边界、失败原子性、测试与文档一致 | 未发现 P0–P3 问题；指出 y 越界缺少专门 fixture，已记录为测试限制 |
| 2026-11-09 | 真实窗口桌面走查 | 确认主题背景与用户端视觉效果 | 本轮未执行；Qt Quick 场景图像素测试已验证黄色块层和粉色 inline 层叠加，维护者可在合并后补充实机视觉验收 |

## 风险与回退

Equation/Align 的 MathFragment 坐标相对页面版心；转换成页面内容区域时加页边距，并按所在页面裁切。当前分页器不拆分单个公式块。活动 Align cell 不单独着色，避免用不可见的占位盒推导活动单元格状态。

## 决策与工作记录

- 2026-11-08：用户报告单行/多行数学块缺少奶黄色背景。初查发现 Rust 只从块环境 ranges 发布背景，QML 负责绘制但不能补出公式宿主范围；在 072 完成后单独实施，避免把公式块误建模为环境节点。
- 2026-11-08：用户补充 Equation/Align 中使用 `\text` 时，活动文本岛也需要粉色 inline 高亮；与奶黄色块级高亮分别建模、绘制并验证共存。
- 2026-11-09：实施决策：Align 黄底覆盖整个活动 Align 宿主，文本焦点由粉色活动 TextFlow island 标记；黄底横跨版心内容宽度，与普通块高亮一致。当前单个数学块不跨页拆分，验收以实际 MathFragment 所在页及页面裁切为准。
- 2026-11-09：开始实施前审计发现现有 Canvas 高亮颜色/几何只有单一列表、inline 覆盖 block；MathEdit Block 的正文停靠点还可能落在相邻 Section。新公式块几何独立投影，公式活动时禁止从该停靠位置产生普通块环境黄底。
- 2026-11-09：按用户要求调整复用边界：普通块环境和 MathFragment 仍分别由领域快照投影，但在 ViewModel 汇入同一个黄色块高亮列表，Section 与 Equation/Align 走同一个块矩形校验和 Canvas 场景图实现；inline 粉色高亮为叠加层。

## 完成摘要

Equation/Align 的块级高亮与数学 TextFlow 行内高亮已实现；普通环境高亮同步迁移到统一的块/行内列表和 QSG 绘制层。Rust、Qt ViewModel、Canvas/QML 行为及文档契约均有验证，独立评审无阻断发现。真实窗口桌面走查未执行，但 Qt Quick 像素测试已验证两层实际绘制，因此不阻塞本 task 验收；数学文本 y 越界尚无专门 fixture，作为已记录的测试限制保留。
