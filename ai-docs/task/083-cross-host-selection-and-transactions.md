# 083 — 跨宿主选区、编辑事务与几何投影

- 状态：planned
- 依赖：071、074、077、081、082
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

数学块与 InlineMath 目前不能完整进入文档级选区；Shift+方向键部分路径无操作，跨块范围、删除恢复、复制和高亮几何缺少单一协议。数学块删除可能跳过相邻块，Section 前 Backspace 可能产生 `edit.invalid_position`。083 使用 081 的三态焦点和 082 的文档位置/顺序，统一选区、编辑事务及同代布局投影。

## 必读

- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [行内与块级环境视图](../architecture/editor/document-model/environment-views.md)
- [画布与交互背景](../architecture/interface/canvas-highlighting.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 让 Paragraph、Section/Custom、Equation/Align、InlineMath、InlineEnvironment 与数学文本流共用 anchor/focus 范围、方向、替换/删除目标和位置映射；局部坐标由 adapter 映射。
- Shift+四方向键构造同一文档顺序下的选区；数学树/源码/文本岛内部按本地语义移动，跨宿主边界转换为可操作的结构范围。
- 选区删除、替换、复制、Undo/Redo 与失败原子性消费相同范围，不泄漏 U+FFFC、不静默丢失节点或恢复过期视觉选择。
- 修复连续 Equation/Align 删除落点、Section 标题/首段邻接 Backspace 失效及块/Inline 选区无法表达等 080 原始问题。
- 完整快照、交互刷新与 Qt 几何组合复用共享 Environment/selection projection；允许 Rust/Qt API 经评估调整，不预设具体 rect 数组结构。
- 审计 `current_inline_environment()` / `InlineEnvironmentRange` 与 `CurrentEnvironmentView` 的职责重叠、公共消费者和兼容边界；先统一语义投影，再按调用方证据决定合并、弃用或保留适配 API。
- 审计 `selected_block_environment` 与 `selected_block_environments` 的写入路径、外部消费者和空块外删除特例；将单值从多选权威状态派生，或保留有明确语义的独立状态，并覆盖失败回滚。
- 审计数学 projection identity 在历史合并与重绑中的比较；只合并重复比较，不改变 Undo/Redo 归组或目标宿主恢复。
- Table/Figure 仅遵循共享协议扩展点；不实现单元格网格选择或图形编辑。保留 079 已定义的 Align 行首退格规则。

## 前置条件与待决策

- Task 081 的三态焦点仲裁和 Task 082 的宿主全序/定位已完成。
- 复制/替换数学对象的公开命令语义按现有产品契约核对；不得因 UI 暂无命令而将选中状态与可编辑范围拆成两套权威。

## 实施步骤

1. 将 Shift+Left/Right/Up/Down、鼠标选择及选区折叠映射到 082 的共享文档位置；统一普通、MathText、MathTree/Source 与结构范围的 anchor/focus。
2. 让结构删除/替换、复制和历史恢复消费同一范围；保持单个公式对象删除与结构选区删除各自的业务目标，但共享位置映射与事务底座。
3. 修复块数学连续删除落点和 Section 邻接 Backspace；验证结构状态从真实宿主路径解析，不使用隐藏 Paragraph 锚点。显式 Escape 离开数学时继续跳过连续 Equation/Align 并寻找最近的同级 Paragraph；对象删除只移除目标对象，不复用 Escape 的连续公式链落点规则。
4. 让活动 Inline/Block 环境、高亮、命中与选区矩形由同一语义投影生成，并由完整快照、增量刷新和 Qt 使用同代结果。
5. 覆盖跨 Paragraph、Block、InlineMath、嵌套 Section、反向选择、空槽位、连续公式链 Escape 与单对象删除、失败/Undo/Redo 的黑盒和领域用例。
6. 运行影响范围内验证，委派 Terra/medium 分域评审；修复后由未参与实现的 Sol/medium 对最终 diff 独立只读评审。

## 预计改动

预计涉及 `caret-engine` Session selection/transaction/history、`caret-structure` 范围消费、`caret-app` 快照与 hit/navigation 候选、`caret-layout` 几何映射及 Qt selection/highlight 投影和跨层行为测试。不要把导航仲裁复制到 Qt/QML，不为每个结构 kind 新建平行选区实现。

## 验收标准

- [ ] Shift 四向键可在 Paragraph、复杂 Block 和 InlineMath 两侧扩选，anchor/focus 方向和文档范围一致；内部 MathTree/MathText 位置可正确映射。
- [ ] 显示选中的范围可被删除/替换/复制，U+FFFC 和节点数据不丢失；成功、失败、Undo、Redo 后 caret、selection、历史状态有效。
- [ ] 连续 Equation/Align 删除只移除目标宿主并落到最近有效边界；Section 邻接 Backspace 不产生 InvalidPosition，拒绝时无副作用。
- [ ] Escape 沿既有语义越过连续 Equation/Align 查找同级 Paragraph；删除单个公式仅删除目标且采用独立的最近有效位置恢复。
- [ ] 完整快照、增量刷新与 Qt 绘制复用同一 Environment/selection 语义投影和 document generation；Inline/Block 几何来源由 adapter 保留。
- [ ] 公开环境查询 API 的调用方已盘点；兼容性与迁移路径有依据，不保留重复权威语义，也不因内部整理破坏外部消费者。
- [ ] Block 环境单值/多值选择状态及 Math projection identity 的消费者和历史语义已审计；无丢失空块删除、失败回滚或 Undo/Redo 行为。
- [ ] 不存在可见选择但空文档范围、隐藏 selection、相邻数学块被跳过或旧 generation 高亮覆盖新状态。
- [ ] 081/082 的定位和三态协议无重复权威；新 Block 通过 adapter 接入而不复制键盘/选区仲裁。
- [ ] 相关行为测试、格式/lint、覆盖率门槛和独立评审通过；未执行的平台验收明确记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Shift 四向键跨 Block/Inline 选择测试 | anchor/focus 与结构范围一致 | 未执行 |
| — | 删除/替换/复制/Undo/Redo 跨宿主测试 | 同一范围，无数据丢失 | 未执行 |
| — | 同代 Environment/selection 几何快照测试 | 完整/增量投影一致 | 未执行 |
| — | Terra/Sol 独立审查 | 无路径重复、失焦或历史回归 | 未执行 |

## 风险与回退

文档位置和布局快照代际不一致会使范围落错对象；结构删除的焦点恢复还可能跨越相邻块。每次修改前校验 generation、宿主身份和可排序范围；映射失败则原子拒绝并保留文档与有效状态。

## 决策与工作记录

- 2026-11-09：从 080 拆分 Issue #126 的选区、删除、环境投影问题及原始三类用户缺陷；依赖共享三态和全序定位，不做逐类型补丁。

## 完成摘要

未完成。
