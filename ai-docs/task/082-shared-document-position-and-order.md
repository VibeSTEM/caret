# 082 — 共享文档位置、顺序与宿主定位

- 状态：in-progress
- 依赖：071、074、077、081
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-11

## 目标

在 081 的 Paragraph/Block/Inline 宿主位置协议上，建立经过校验的物理父链、宿主间顺序和局部位置比较；供 083 的选择与编辑命令使用。消除任取 Paragraph 当隐藏锚点、重复实现父链遍历，以及把稳定 ID 或段落 ordinal 当全树顺序的做法。082 不重新定义 081 的焦点载体。

## 相关权威文档

- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [文档树与结构](../architecture/editor/document-model/document-structure.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与规则

- 顺序从当前文档的真实结构、slot 与已验证局部坐标推导；ID 只标识节点，Paragraph ordinal 只服务段落导航。
- 比较前验证 Session、document generation、宿主/目标类型、Story、父链、slot、路径、偏移和字素边界；找不到唯一映射时原子拒绝。
- `StructureIndex` 的大纲父链和文档树物理父链语义不同，必须分别维护。
- `ExplicitBreak` 与 `Opaque` 占据物理 slot，但不因此获得持久 ID 或编辑能力。
- 只合并已证明重复的状态/定位逻辑；保留 Undo/Redo、未知字段、schema 版本及外部序列化契约。
- 不实现跨宿主选择、复制、删除命令和完整几何投影（083）；不宣称未经基准验证的复杂度收益。

## 当前实现与明确边界

- `caret-structure` 提供稳定 ID 节点的物理位置、父环境链、真实同级 slot、节点边界顺序，以及按父环境 ID 解析只读/可变块列表的共享入口。父 ID 在每一级只匹配当前块序列的直接 Environment；整树唯一 ID 由已验证的 Document 契约保证。`math_edit`、`structure`、`history` 继续负责各自的错误转换、目标校验、快照匹配与事务回滚。
- `caret-engine` 的 Session 比较器覆盖段落文本、块与行内宿主边界、Section 内段落、Equation/Align 内部与周边位置、InlineMath、MathTree/MathTextFlow，以及同一树内的嵌套操作数文本位置。比较沿文档树/MathTree 的实际结构顺序；同一连续数学文本岛使用展平后的字素顺序和既有 affinity 规则。
- 空段落、空 Inline、相邻宿主共享边界和相邻文本岛保留可区分的 affinity；重叠、边界映射不唯一或坐标无效时不猜测次序。
- MathTree 操作数按语义槽位排序，不代表所有节点的 LaTeX 源码顺序。解析器的 SubSup `x^a_b` 中，上标源码在下标前，而树槽位仍为下标后上标。故 `SourceUtf8` 与 Tree/Text 混合比较继续拒绝，直到 adapter 能证明同代投影、唯一映射及两种顺序的关系。
- 样式映射、主题命名样式和媒体等字段审计结论见“持久模型字段审计”；当前没有证据支持删除这些 wire 字段。

## 持久模型字段审计

| 字段 | 消费者或兼容依据 | 决定 |
| --- | --- | --- |
| `DocumentSettings.defaultStyles`、`formatOverrides`；`ThemeSnapshot.namedStyles` | 当前无运行时样式解析，但它们是已知 DTO/wire 字段；存储和 App 打开、保存、重开测试验证往返。类型化 map 的缺省值与类型校验不同于 `extra`。 | 保留显式字段；未来样式消费者待相关设计处理。 |
| `CdocFile.media` / `Media` | 打开校验唯一非空 ID、非空类型、标准 Base64 和 32 MiB 解码上限；Session 保存快照携带媒体，测试验证未知元数据往返。图片解码/尺寸校验属于 task 052。 | 保留。 |
| Paragraph `language` / `layout` / `markStyle` / `props` / `extra` | shaping、分页、合并兼容性、`outlineLevel` 索引、未知字段保留和拒绝有损编辑均有消费者。 | 保留。 |
| Inline/Block `definitionId` | 两类均序列化；Inline 参与格式/数学模板语义，Block 当前仅持久化并推导 capability。打开 `.cdoc` 仅在能力和主题版本可完整校验时调用完整文档验证，否则进入安全预览；直接 `Document::validate()` 始终完整验证。 | 保留，等待 Block 模板语义设计。 |
| 节点 ID、Opaque/`extra`、格式/主题/MathTree schema version | 唯一性、物理定位、编辑/历史身份、未知内容往返、安全拒绝有损变换及版本兼容均依赖这些字段。 | 保留；不为统一位置 key 给 Opaque/ExplicitBreak 增加 ID。 |

## 实施顺序与剩余工作

1. 完成剩余合法 caret 组合的统一比较，明确哪些坐标因 carrier 或结构歧义应拒绝。
2. 继续审计 Session/历史位置字段的构造、读取、快照、投影与生命周期；合并有证据的冗余，不删仍承载兼容或扩展契约的字段。
3. 覆盖事务、Undo/Redo、删除、拆分、合并、跨容器移动后的有效映射与原子拒绝；核对共享 locator 没有绕过当前路径/slot/内容验证。
4. 完成本任务剩余模型字段消费者审计；记录性能基线前不作复杂度承诺。
5. 收敛后跑全 workspace 验证，并按验证规范完成 Terra 领域复核和独立 Sol final review。

## 验收标准

- [ ] 嵌套块、标题、空容器、连续公式块、InlineMath 和 Paragraph 文本之间的所有可证明位置有一致顺序；稳定 ID 和 ordinal 不充当排序键。
- [x] 活动结构父链可按有效宿主 ID 查询；不从任取 Paragraph 推断 Block 状态。
- [ ] locator 共享身份、父链、slot 和 generation 校验；只读与可变入口都经当前文档验证。
- [ ] Session/历史位置字段有完整消费者和生命周期证据；可合并的重复状态已收敛。
- [x] `defaultStyles`、`formatOverrides`、`namedStyles` 与媒体候选字段已审计并有保留理由；模型字段往返与消费者审计其余项仍需完成。
- [ ] Undo/Redo、删除、拆分、合并及跨容器移动后的映射有效；失败时原子拒绝且保留文档/Session 状态。
- [x] ExplicitBreak/Opaque 的物理 slot 与无稳定 ID 语义已核对，不为排序添加持久 ID。
- [ ] 代表性性能基线完成；无基线前不宣称性能提升。
- [ ] 最终相关测试、lint、格式和独立评审通过；全 workspace 验证须在本轮代码改动结束后重跑。

## 验证记录

- **2026-11-11 当前分支定向验证：** `cargo test -p caret-structure --locked`（13 项）、`cargo test -p caret-engine --locked`（752 项）、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff HEAD --check` 均通过。覆盖跨岛/嵌套 MathTree 顺序、混合源码/树 carrier 拒绝、父环境 ID 只读/可变路径和历史/结构/数学编辑调用方。
- **2026-11-10 workspace 基线：** `cargo test --workspace --locked` 通过（engine 744、structure 10、App 285、native/QML 集成 11 及其他 crate/doc tests；1 项受控 crash 子进程测试 ignored），workspace Clippy、格式和差异检查通过。此后增加了比较器与父链实现，本轮完成前需重跑 workspace 测试。
- 模型往返专项：`cargo test -p caret-app -p caret-doc --locked` 当时通过（App 285、Doc 78）；性能基线未执行。

## 风险与回退

错误顺序会影响焦点、Shift 范围和删除落点；陈旧 locator 会指向已移动或已删除节点。任何不能验证唯一映射的坐标都必须拒绝。共享父链查询只解析当前物理路径，不代替 ParagraphLocation 的 Story、文本偏移、document generation、目标 slot 和事务快照校验。
