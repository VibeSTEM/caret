# 082 — 共享文档位置、全序与宿主定位

- 状态：in-progress
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
- [x] 按任意有效宿主 ID 查询到正确父链和同级次序；活动结构状态不从任取 Paragraph 推断。
- [ ] locator 统一共享身份、父链、排序与 generation 校验；只读查询与可变事务定位都使用正确且经验证的入口。
- [ ] 位置/Session/历史字段审计有消费者和生命周期证据；冗余状态得到收敛，必要的扩展与兼容字段保留理由明确。
- [ ] 持久模型字段逐项有序列化与领域消费者依据；拟删除字段已验证所有构造/读写/round-trip/兼容路径，未删除字段记录其扩展或未知数据契约。
- [ ] `defaultStyles`、`formatOverrides`、`namedStyles` 和顶层媒体资源等候选字段均有完整消费者与调用方证据；仅在证明无契约、无消费者后删除。
- [ ] Undo/Redo、删除、拆分、合并和跨容器移动后的映射保持有效；无法映射时原子拒绝并保留文档与会话状态。
- [x] 不因排序实现需要就为 ExplicitBreak/Opaque 添加 schema ID；若新增身份有明确需求和往返/兼容/历史证据，记录独立决策。
- [ ] 性能目标有代表性文档与可复现基线；未测量时不宣称复杂度改进。
- [ ] 相关格式/lint、行为测试和独立评审通过；未执行验证明确标记。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-10 | `cargo test -p caret-structure --locked` | StructureIndex 导航语义与物理父链/slot 顺序回归通过 | 8 项通过；物理节点顺序可查，但尚未覆盖任意 Paragraph caret 与 InlineMath 局部位置的全序 |
| 2026-11-10 | `cargo test -p caret-engine --locked` | locator、撤销重做与失效映射回归通过 | 710 项通过；复合历史补丁部分失配及同 ID 内容变化时原子拒绝 |
| 2026-11-10 | 段落位置比较的 Story、locator 与 TextMap 边界回归 | 无效位置不参与排序，段落内 text/InlineMath 占位偏移保持真实顺序 | wrong Story、越界 offset 和失效 block path 定向测试通过；通用宿主 caret 全序仍未实现 |
| 2026-11-10 | `cargo test -p caret-engine --locked` 与 `cargo clippy -p caret-engine --all-targets --locked -- -D warnings` | 校验后的段落位置排序调用链通过 | 713 项测试与 Clippy 均通过；覆盖 InlineMath 占位符前后、标题在子段落前、跨段落 IME 选区及无效 Story/offset/path |
| 2026-11-10 | `cargo test -p caret-app -p caret-doc --locked` | 文档编辑与格式/未知数据往返通过 | 285 项 App、78 项 Doc 通过；包含打开、编辑、保存、重开后保留样式映射、主题命名样式和媒体 |
| — | 代表性文档定位基准 | 提供性能基线，不先定复杂度结论 | 未执行；不宣称性能改进 |
| 2026-11-10 | `cargo format --check`、`cargo quality` | 格式、lint、依赖审计与全 workspace 测试通过 | 通过；cargo quality 在沙盒外运行以访问只读保护的 cargo advisory 数据库 |
| 2026-11-10 | Terra 领域审查与 Sol 最终审查 | 共享定位语义正确，无身份回归 | Terra 分领域初审完成；Sol 对完整 staged diff 复审通过，历史同 ID 失配发现已修复并补回归 |

## 风险与回退

错误文档顺序会影响焦点、Shift 范围和删除落点；错误 locator 会指向已移动或已删除节点。任何无法校验的路径都必须拒绝，不回退到任意 Paragraph 或按 ID 字典序猜测。

## 决策与工作记录

- 2026-11-09：从 080 拆分 Issue #126 的 H1、M1、M3、M4 相关工作；不照搬固定 enum、直接改 `StructureIndex` 为可变索引、给 Opaque/ExplicitBreak 统一加 ID 或未测 O(深度) 承诺。
- 2026-11-10：实现前审查确认 `StructureIndex` 的 heading 父链表示大纲语义，不是文档树物理父链；新增/复用定位契约时必须保留两者的区别。物理路径保留每一层真实同级 slot，包括没有稳定 ID 的 ExplicitBreak/Opaque；这些节点不得因此获得持久 ID 或编辑能力。
- 2026-11-10：第一方持久字段消费者审计未发现可安全删除字段。未读取的样式映射、主题命名样式和 definition ID 仍属公开 DTO / `.cdoc` 兼容契约；后续补非空值打开、编辑、保存、重开验证，不以仓内无语义消费者推断字段无用。
- 2026-11-10：物理定位入口落在 `caret-structure`，保留 `StructureIndex` 的大纲语义；公式块路径和活动环境父链改用宿主 ID 查询，Paragraph 缓存与历史路径核对完整父环境链及真实 slot。无 ID 的显式分页和 Opaque 仍只占 slot，不创建持久身份。
- 2026-11-10：历史补丁重放在候选文档上事务应用；复合补丁中任何一项路径、slot 或快照不匹配时拒绝整个撤销/重做，不推进 Session 历史游标。Paragraph 快照删除要求 ID 与完整值同时匹配。
- 2026-11-10：段落位置比较现校验当前文档上的目标 ID、Story、完整物理路径及 TextMap 合法边界后才排序；Story 错配、越界偏移和陈旧路径均拒绝。此比较仅覆盖 Paragraph 文本位置，不能代表 Inline/Block 宿主状态或数学局部位置的通用全序。
- 2026-11-10：尚未实现通用 caret 全序，尤其是同一 Paragraph 中文本偏移与 InlineMath/InlineEnvironment 结构边界之间的排序；共享物理节点顺序不能替代宿主局部位置转换。082 保持 in-progress，待补齐该位置层和对应测试。

## 完成摘要

未完成。
