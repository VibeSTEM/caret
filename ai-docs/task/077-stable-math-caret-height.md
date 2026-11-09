# 077 — 数学宿主行框与光标高度统一

- 状态：ready
- 依赖：044、074
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

维护者实测发现，数学块宿主高度会随公式内容变化，数学光标也可能随焦点附近的字符或结构高度跳变。期望的默认宿主行框与正文文本一致：普通数学字符不应各自改变行高；上标、分数、根式等结构超出正文基准行框时，按正文行距类型决定扩展或固定推进，不允许为了稳定高度而压缩公式。

问题包含两个相关但独立的几何结果：数学块/行的宿主行框，以及各数学编辑分支的光标矩形。只统一块高度不会自动统一光标高度。行内公式、Equation、Align、MathTree 与 MathText 应使用一致的基础行度量和扩展规则；公式布局仍负责结构内的 x/y 定位、脚本基线、分数槽位和 Align 行/单元格位置。

初步代码审计显示，普通段落由 `caret-layout` 的行框计算结合字体行网格、行距和行内对象确定高度，正文光标消费所属行高度；Equation/Align 当前直接用 MathLayout 的 ascent + descent 作为 MathFragment 高度。MathTree 光标按相邻结构盒生成，MathText 光标按 Parley 文本岛光标盒生成，二者绕过正文 caret 行框。实现前须核实实际行为并确认共享度量的权威边界，不假定当前已有 text-only caret 度量。

## 必读

- [数学模型与排版](../architecture/README.md)
- [分层规范](../standards/layering.md)
- [测试规范](../standards/testing.md)
- [验证与评审](../standards/validation-and-review.md)
- [空公式宿主占位](072-empty-math-host-placeholder.md)
- [数学光标候选与统一仲裁](074-math-caret-routing-and-geometry.md)

## 范围与非目标

- 核对普通文本 caret 高度的权威度量和所有数学 caret geometry 生成路径。
- 复用正文行网格、行距与光标基础高度策略；相同正文基础样式、字号和行距下，数学普通字符使用与正文一致的默认宿主高度，Math/Body 字体角色可不同。
- 数学内容超出基础行框时按正文行距类型扩展或固定推进；上标、下标、分数、根式等保留行距规则允许的结构空间，不为了稳定光标而压缩数学内容。
- 统一行内公式、Equation、Align、MathTree、MathText 和源码分支的基础光标高度契约，同时保留各数学结构所需的 x/y 位置、脚本定位和行/单元格选择。
- 删除重复的数学专用高度策略；Qt/ViewModel 只消费 Rust 产生的几何，不再引入第二份高度推导。
- 覆盖基线字形、上下标、`\frac`、`\sqrt`、`\text`、空公式占位符及 Align 多行。
- 不改变数学公式本身的排版、上下方向键候选、光标横向命中、占位符大小或普通文本 caret 行为。
- 不以系统输入法提示或光标闪烁/可见性控制代替几何高度契约。

## 前置条件与待决策

- 审计正文段落实际使用的行框和 caret 高度；结构导航架构只保留稳定行为契约，不记录字段名、实现快照或具体 task 追踪。
- 统一正文基准行框的来源和数学宿主扩展规则；行内公式、Equation 与 Align 使用相同策略，不因公式容器种类复制高度算法。
- 数学内容超出正文基准行框时，按[段落行距](../architecture/editor/page-layout.md#行距与默认值)的 Natural、Multiple、AtLeast、Exactly 语义计算行框；不能用焦点字形局部 bounding box 作为整行高度或硬压缩公式。
- 光标高度和宿主行框需分别验证；保留基于 MathTree 结构与 MathText 插入位置的定位信息。调整光标中心或高度时，检查 Up/Down 候选排序和命中阈值。
- 用户补充：数学多行由换行符构成；Enter 可插入额外数学行，但 Backspace 当前不能删除该行边界。Terra 审计发现 Align MathTree Enter 创建结构行，Align Source 插入源码分隔符，MathText Enter 不支持换行，三者不能统称为 `MathContent::SoftBreak`。行边界删除和行合并已拆分到[079 数学多行换行与删除事务](079-math-multiline-break-deletion.md)；本 task 仅在行数变化后验证宿主几何和焦点。

## 实施步骤

1. 绘制正文段落、InlineMath、Equation/Align、MathTree、MathText 和源码分支的宿主行框与 caret 数据流，标出 x、y、基线、行高的唯一来源及重复策略。
2. 增加相同正文基础样式、字号和行距下不同普通数学字符的宿主高度与 caret 高度对比用例，复现字符相关变化；数学字体角色可独立于正文。单独覆盖超出正文基准行框的上标、分数、根式。
3. 抽取或复用正文行框度量作为数学默认值，并按行距类型处理数学内容需求；MathTree、MathText 等 caret 共享基础高度策略，但保留结构定位。
4. 覆盖 inline、equation、align、空宿主、MathTree、MathText、源码及不同数量行的几何，不负责定义行边界编辑事务；用现有 Enter/Undo 路径或独立多行 fixture 验证行数变化，覆盖不同字体/字号/行距、对象溢出、分页、相邻块和 Up/Down 候选。
5. 同步更新光标高度架构文档，删除过期接口描述和重复高度逻辑；运行相关 Rust/native/Qt 测试和可用桌面验证，再由 subagent 独立审查最终 diff。

## 预计改动

预计涉及 `crates/caret-layout/src/line_metrics.rs`、`crates/caret-engine/src/layout.rs` 的数学块与 caret 投影、`crates/caret-engine/src/layout/math_navigation.rs` 的垂直候选、`crates/caret-app/src/document_view/`、Qt ViewModel/Canvas 及对应测试。`page_glyphs.rs` 只在审计确认它是度量策略权威时才纳入；避免在布局、快照、Qt 各层重复存储或推导高度。

## 验收标准

- [ ] 相同正文基础样式、字号和行距下，数学普通字符与正文使用一致的默认行框/光标高度；允许 Math/Body 字体角色不同，且仅字符不同不造成无理由的高度跳变。
- [ ] 上标、下标、分数、根式和 `\text` 超出正文基准行框时，Natural/Multiple/AtLeast 按内容需求扩展；Exactly 保持固定推进并按段落行距规则处理溢出。结构内部 caret 仍定位于正确脚本/分数槽位。
- [ ] 同一基础高度与扩展规则覆盖行内公式、Equation、Align 单元格和空公式占位符；同类内容不因容器实现不同而重复计算高度。
- [ ] caret 的水平位置、上下标所在视觉行、双向文本亲和性及上下导航候选不因高度统一而回归。
- [ ] 不同数量的数学行遵循一致行框/光标度量；通过现有 Enter/Undo 路径或多行 fixture 验证行数变化。
- [ ] Rust geometry、快照序列化、ViewModel/Canvas 投影的行为测试覆盖成功和无效/缺失 geometry。
- [ ] 可用目标桌面环境验证真实 caret 显示；未运行的平台验证如实记录。
- [ ] 相关文档、验证记录和独立评审完成，重复高度策略及过期实现已清理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-09 | Terra/medium 只读数据流审计 | 找出正文与各数学分支的行框/caret 来源、重复策略及风险 | 已完成；确认 Equation/Align 宿主直接用 MathLayout ascent + descent，MathTree/MathText caret 独立取结构盒/文本岛盒；指出结构导航文档与正文 caret 现状不一致，并区分 Align Tree 行、源码分隔符与 SoftBreak；未执行行为复现或测试 |
| — | 数学 caret geometry 回归测试 | 同一行不同焦点字形高度一致 | 未执行 |
| — | Rust / native 相关测试 | 快照及 Qt 投影保留稳定高度 | 未执行 |
| — | 桌面可视验证 | 所有数学宿主光标高度稳定且位置正确 | 未执行 |

## 风险与回退

统一基础高度可能导致复杂公式超出当前行距容纳范围。按 `page-layout.md` 中各行距类型处理：Natural/Multiple/AtLeast 按内容需求扩展；Exactly 保持固定推进并允许溢出裁切。不能通过偷偷更改 Exactly 语义来隐藏溢出。

## 决策与工作记录

- 2026-11-09：按用户确认创建独立任务，覆盖全部行内与块数学环境及 MathTree/MathText/源码光标状态。
- 2026-11-09：维护者实测补充数学块宿主高度随字符/公式内容变化，希望基础高度与正文一致，脚本和分数等按实际内容扩展。Terra 初步只读审计确认 Equation/Align 的 MathFragment 直接采用 MathLayout ascent + descent，MathTree 与 MathText caret 使用独立几何；因此把宿主行框和 caret 高度列为分别验收的关联问题。
- 2026-11-09：Terra 发现原结构导航文档对正文 caret 的实现描述不符合当前代码；按维护者要求，架构页现只保留目标行为契约，077 实施时仍需从代码与行为测试确认可复用的正文基准。
- 2026-11-09：维护者补充 Align Enter 可创建额外数学行但 Backspace 无法删除行边界。Terra 只读审计确认 Tree 行边界由 MathAlignRow 表达，空 cell 退格当前只移动到相邻 cell；源码行分隔符已有删除测试。为保持职责清晰，删除/合并契约单独建入 079，077 只覆盖现有 Enter/Undo 路径或 fixture 上的行数变化几何。

## 完成摘要

未完成。当前完成 Terra 只读数据流审计并更新了架构行为契约；尚未实现或添加复现性测试。
