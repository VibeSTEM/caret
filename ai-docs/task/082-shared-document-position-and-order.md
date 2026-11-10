# 082 — 共享文档位置、全序与宿主定位

- 状态：planned
- 依赖：071、074、077、081
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-10

## 目标与背景

段落 ordinal 不能表示连续 Equation/Align 与周边 Paragraph 的完整次序；数学块没有 Paragraph 停靠点，嵌套 Section、标题及空容器也不能用一个 Paragraph 锚点代替。现有 `find_block_path`、ParagraphLocation 和历史恢复 locator 重复实现，查询与变更的适用范围不同。082 消费 081 已定义的宿主身份、三态类别和宿主局部位置载体，在此基础上建立共享文档顺序、父链与经过校验的定位契约，供 083 使用；不重新定义或替换 081 的焦点载体。

## 必读

- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [文档树与结构](../architecture/editor/document-model/document-structure.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 在 081 的宿主相对三态位置载体上定义跨宿主父链、同级次序、全树顺序、Story 边界、document generation、失效映射和原子拒绝语义；外前/外后及内部位置的宿主身份和局部坐标继续遵循 081。
- 为 Paragraph、Section 标题/子树、Equation、Align、InlineMath 和文本 Inline 提供可比较的文档顺序；段落 ordinal 只作为 Paragraph 导航派生值。
- 合并/重用 ParagraphLocation、`find_block_path`、历史恢复等 locator 的共同定位语义；只读查询与可变事务路径各自保留适当职责，并共享身份、父链、generation 校验。
- 审计位置、Session 与历史相关字段的构造、读取、投影和调用方；合并或删除有证据确认的重复/无消费者状态，同时保留稳定身份、历史语义和已有扩展契约。
- 审计持久模型字段的序列化、布局、编辑、未知字段与扩展消费者，包括 Paragraph 的 language/layout/style/props/extra、`DocumentSettings.defaultStyles` / `formatOverrides`、`ThemeSnapshot.namedStyles`、顶层媒体资源、环境 definition ID、节点 ID、Opaque 与各层 schema version；核对所有 crate 和外部构造调用方，不能仅因单一路径未读就删除。
- 消除通过 ParagraphIndex 任取正文段落作为隐藏锚点、再从其祖先链推断活动 Block 的路径。环境状态须从有效宿主身份解析父链。
- 审计 `ExplicitBreak` 与 `Opaque` 的定位需求；不为满足统一 key 形状而强制增加持久 ID，不把 Opaque 当作可编辑原子。
- 不实现跨宿主选择/复制/删除命令与完整几何投影（083）；不预设 O(深度) 等未经基准验证的复杂度承诺。

## 前置条件与待决策

- Task 081 的共享 Block/Inline 三态焦点协议及宿主相对位置载体完成；本任务只扩展跨宿主顺序与定位。
- 具体 Rust enum、排序 key 编码与 locator 模块边界由实现前设计决定；架构契约只规定可观察的身份、顺序、代际与失败行为。

## 实施步骤

1. 绘制现有 Pos、MathEdit、Block 外侧位置、StructureIndex、ParagraphIndex、ParagraphLocation 与历史 locator 的关系，标明共享值和各自语义。
2. 设计并实现可比较的宿主位置和文档顺序；覆盖嵌套 Section、title、连续/空公式块、InlineMath、同 ordinal 条目及不可排序 Story。
3. 建立按宿主 ID 查询父链的只读入口和经验证的可变事务定位入口；修复任意 Paragraph 隐藏锚点回退。
4. 收敛重复路径查找，校验定位结果随事务、撤销/重做、移动、文档代际变化的有效性。
5. 对 ExplicitBreak/Opaque 做稳定身份、未知数据 round-trip 和历史生命周期核查；仅有明确能力需求时调整模型。
6. 对候选无消费者字段逐项搜集构造、序列化、布局、编辑、扩展及调用方证据；只有确认不属于公开/外部契约且删除不破坏往返语义时才移除。
7. 为文档顺序与 locator 增加正反向、无效位置、事务回滚和重复构造等行为测试；完成 Terra/Sol 独立审查。

## 预计改动

预计涉及 `caret-structure` 文档索引与宿主定位、`caret-engine` Session/paragraph/history locator、必要的模型 adapter 及 Rust 行为测试。除非证明稳定独立职责，不新建 crate，不变更 `.cdoc` schema。

## 验收标准

- [ ] 有效文档位置在嵌套块、标题、空容器、连续公式块、InlineMath 与 Paragraph 文本之间可确定顺序；稳定 ID 只作身份，Paragraph ordinal 不充当全树排序键。
- [ ] 按任意有效宿主 ID 查询到正确父链和同级次序；活动结构状态不从任取 Paragraph 推断。
- [ ] locator 统一共享身份、父链、排序与 generation 校验；只读查询与可变事务定位都使用正确且经验证的入口。
- [ ] 位置/Session/历史字段审计有消费者和生命周期证据；冗余状态得到收敛，必要的扩展与兼容字段保留理由明确。
- [ ] 持久模型字段逐项有序列化与领域消费者依据；拟删除字段已验证所有构造/读写/round-trip/兼容路径，未删除字段记录其扩展或未知数据契约。
- [ ] `defaultStyles`、`formatOverrides`、`namedStyles` 和顶层媒体资源等候选字段均有完整消费者与调用方证据；仅在证明无契约、无消费者后删除。
- [ ] Undo/Redo、删除、拆分、合并和跨容器移动后的映射保持有效；无法映射时原子拒绝并保留文档与会话状态。
- [ ] 不因排序实现需要就为 ExplicitBreak/Opaque 添加 schema ID；若新增身份有明确需求和往返/兼容/历史证据，记录独立决策。
- [ ] 性能目标有代表性文档与可复现基线；未测量时不宣称复杂度改进。
- [ ] 相关格式/lint、行为测试和独立评审通过；未执行验证明确标记。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | StructureIndex 全树顺序与父链测试 | 顺序和祖先与文档树一致 | 未执行 |
| — | locator 的 mutation/undo/generation 测试 | 位置更新有效，失败原子 | 未执行 |
| — | 代表性文档定位基准 | 提供性能基线，不先定复杂度结论 | 未执行 |
| — | Terra/Sol 独立审查 | 共享定位语义正确，无身份回归 | 未执行 |

## 风险与回退

错误文档顺序会影响焦点、Shift 范围和删除落点；错误 locator 会指向已移动或已删除节点。任何无法校验的路径都必须拒绝，不回退到任意 Paragraph 或按 ID 字典序猜测。

## 决策与工作记录

- 2026-11-09：从 080 拆分 Issue #126 的 H1、M1、M3、M4 相关工作；不照搬固定 enum、直接改 `StructureIndex` 为可变索引、给 Opaque/ExplicitBreak 统一加 ID 或未测 O(深度) 承诺。

## 完成摘要

未完成。
