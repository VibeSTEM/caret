# 080 — 数学块边界删除与 Shift 选区

- 状态：in-progress
- 依赖：071、074、077
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

维护者报告了数学宿主边界的三类问题，尚需在当前代码上补充精确复现并确认调用路径：

1. 删除一个块级数学环境后，焦点恢复会越过相邻的其他数学块，落到更早的 Paragraph 末尾。连续数学块应保留各自的前后边界状态；删除只移除目标宿主，焦点落在删除位置最近且仍有效的边界，不应为了寻找 Paragraph 而跳过相邻数学块。
2. 数学块前存在 Section 时，在块边界按 Backspace 会报告 `error:text.delete_backward: edit.invalid_position`。块删除/退格仲裁必须遵循 Section 标题、子块和段落的有效位置模型，操作成功或安全拒绝都不能产生非法焦点。
3. Shift+方向键不能选中块级数学环境，行内 `InlineMath` 也不能可靠加入扩展选区。普通文档选区应可沿四个方向跨过数学宿主；数学对象在正文结构选区中的原子范围、anchor/focus 和高亮几何必须一致。

这些是用户观察到的现象，根因和边界例外仍待复现确认。任务完成后，数学块删除、Section 邻接和 Shift 扩选都遵循统一的宿主边界与结构选区模型，不在各数学分支重复实现。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- [结构导航](../architecture/editor/structure-navigation.md)、[数学架构](../architecture/editor/math.md)
- [宿主焦点协议与键盘意图路由](071-host-focus-and-key-routing.md)、[数学光标候选与统一仲裁](074-math-caret-routing-and-geometry.md)、[数学宿主行框与光标高度统一](077-stable-math-caret-height.md)

## 范围与非目标

- 修复连续块级 Equation/Align 的删除落点，保留块链上每个仍存在的左右边界 stop；删除焦点恢复基于相邻有效宿主，不以“找到 Paragraph”为唯一目标。显式 Escape 离开公式仍保留跳过公式链、寻找正文的既有契约。
- 修复数学块紧邻 Section 时 Backspace 的位置解析与失败原子性；覆盖 Section 标题、首个正文 Paragraph、嵌套子块及相邻数学块的有效边界。
- 让 Shift+Left/Right/Up/Down 能扩展正文结构选区并完整纳入/移出块级数学对象和行内 InlineMath；普通光标移动仍使用既有导航规则。数学内部 MathTree/MathText 选区保持其树内语义，只有越过宿主边界才进入正文结构范围。
- 统一数学宿主到正文结构位置的映射、扩选边界和几何来源。结构选中的数学节点必须参与范围投影与 Backspace/Delete 替换；复制行为不得泄露 U+FFFC 或静默丢弃已选数学节点，需实现已有剪贴板表达或明确拒绝不支持的跨层复制。
- 不改变 Align 逻辑行的空行删除/非空行首 no-op 规则（079）；不实现表格单元格选择或一般任意嵌套块的完整结构选择。

## 前置条件与待决策

- 已确认 `Session::delete_math_object` 误用 `paragraph_beside_math_chain`：该 helper 跳过同级 Equation/Align，遇到 Section 返回 None；保留它用于显式 Escape 的原契约，删除改用独立的相邻宿主边界解析。
- 已确认 `StructureIndex` 当前仅为 Equation/Align 建零长 paragraph ordinal 范围，扩选/高亮/删除只按段落及块环境建模；布局候选也按相邻 Paragraph 生成，故只改方向键 handler 不足。
- 已确认 MathTree Tree/Source 的 Shift 水平移动无操作，MathText/MathTree hit 和垂直候选在 extend 分支也会 no-op；MathTree 复制尚无实现。完成前须为 selection 的文档范围、几何、删除/替换和复制定义一致契约。
- 先补运行时可复现测试，分别覆盖 Equation、Align、InlineMath 与 Section/连续数学块组合；记录起始 caret stop、按键方向、预期 anchor/focus 和实际结构位置。
- 明确跨块结构选区应将公式对象视为一个原子范围；当选区从 MathText 文本岛内部发起时，先按公式内部文本规则扩展，越过宿主边界后再进入正文结构选区。
- 若某个焦点状态的 anchor 无法在当前文档位置模型无损表达，操作必须保持原文档和有效焦点，并给出确定行为；不得生成 `InvalidPosition`。

## 实施步骤

1. 为三个现象添加可复现的 Session 行为测试，精确断言结构焦点、选区范围、文档、generation 和历史状态。
2. 追踪数学块删除、Section 邻接 Backspace、左右键结构扩选的完整入口与候选映射，整理重复或互相矛盾的宿主边界规则。
3. 在现有共享 Session 边界/结构选择路径中修复位置解析和扩选范围；Equation、Align、InlineMath 仅提供自身稳定的宿主范围与几何适配。
4. 覆盖连续块、Section 嵌套、块首/尾、相邻段落、反向选择、MathText 边缘、空/非空数学对象、撤销/重做及失败原子性；更新权威架构文档。
5. 运行受影响验证，执行 Terra/medium 分领域检查，并在修复后由未参与实现的 Sol/medium subagent 对最终完整 diff 只读评审。

## 预计改动

预计涉及 `caret-engine` Session 数学删除与结构选区、`caret-structure` 文档顺序/范围索引、`caret-layout` 选区几何、`caret-app` 同代导航候选和剪贴板适配、跨层行为测试，以及数学与结构导航架构文档。Qt/QML 不重写选区仲裁；只有 Rust 投影无法表达必要候选或几何时才修改视图边界。

## 验收标准

- [ ] 删除连续 Equation/Align 链中的任一目标，只移除该宿主；焦点落在删除缝隙最近的仍有效边界，不跳过相邻公式去找更早 Paragraph。
- [ ] 数学块与 Section 标题、首个正文段落或嵌套子块相邻时，Backspace 不产生 `InvalidPosition`；删除、边界停留或安全无操作符合文档结构，失败时状态原子不变。
- [ ] Shift+方向键能从数学块前后扩选 Equation/Align，并能从两侧扩选行内 InlineMath；四个方向、反向 anchor/focus 和跨 Paragraph 情况符合统一选区范围。
- [ ] 从 MathText 内部扩选时先保留数学文本岛内的文本语义；越过公式宿主边界后将数学宿主映射为正文结构选区，不丢失正文锚点。
- [ ] 每个视觉高亮、命中、caret stop 和 anchor/focus 均来自同一代布局几何；不会显示选中但模型范围为空，或模型选择了公式但没有对应高亮。
- [ ] 对数学结构选区执行 Backspace/Delete、输入替换及复制时，文档节点范围与可见选区一致；复制不会输出 U+FFFC 或静默漏掉公式。
- [ ] Undo/Redo 恢复删除前后的有效宿主焦点与选区；所有失败路径保留文档、generation、选区、焦点和历史。
- [ ] 不重复实现 Equation、Align 与 InlineMath 的边界仲裁；旧的跳过链式块和失效位置逻辑已移除，架构文档记录权威调用链。
- [ ] 相关 Rust 行为测试、格式/lint 与独立评审通过；未执行的窗口/平台验收如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Equation/Align 连续块删除与 Section 相邻 Backspace 行为测试 | 焦点有效且只处理目标结构 | 未执行 |
| — | Shift+四向键跨块级与行内数学选区测试 | anchor/focus、结构范围和高亮一致 | 未执行 |
| — | Rust 测试、格式/lint、Terra 检查、Sol final review | 修复符合架构契约且无回归 | 未执行 |

## 风险与回退

块边界既承担插入新段落的位置，也参与结构选区与删除后的焦点恢复。若统一范围映射错误，可能影响相邻 Section 或普通 Paragraph。每项修复应由位置模型驱动并覆盖无相邻段落、连续块、嵌套 Section 和反向选区；发现模型无法表达的边界时应保留焦点状态并原子拒绝，不进行猜测性跳转。

## 决策与工作记录

- 2026-11-09：创建任务。维护者报告块数学删除落点跳过相邻数学块、Section 前退格产生 `edit.invalid_position`、Shift+方向键不能选中块级和行内数学。
- 2026-11-09：Terra/medium 只读领域审查确认：删除分支误用仅供 Escape 的公式链跳过 helper；Section 侧无相邻 Paragraph 时在 mutation 前失败。Shift 选区还缺结构索引与布局几何对数学块的支持；扩选须同步考虑删除、替换、复制，不能只开放 handler。

## 完成摘要

未完成。
