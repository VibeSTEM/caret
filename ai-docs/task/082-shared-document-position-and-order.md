# 082 — 共享文档位置、顺序与宿主定位

- 状态：done
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
| `DocumentSettings` 的语言、字体、字号、段落默认值；`ThemeSnapshot` 的版本、样式和环境模板 | 字体解析、文本塑形、段落布局、环境样式解析和 capability 扫描直接读取；已知 `defaultStyles`、`formatOverrides`、`namedStyles` 虽暂未运行时解析，仍是已发布 DTO/wire 字段并有打开/保存/重开往返测试。类型化 map 与 `extra` 的兼容语义不同。 | 全部保留；未知主题/字段按当前安全预览及兼容规则处理。 |
| `Document.pageSections` / `PageSection` | 页几何与分页读取；Document 验证要求当前唯一页面节，文件往返与几何原子更新测试覆盖。 | 保留。 |
| Paragraph、Run、TextFlow、SoftBreak | 段落文本/索引、语言 shaping、局部格式、换行、历史和未知字段拒绝有明确消费者；未知 `extra` 往返测试覆盖。 | 保留。 |
| Block/Inline 环境、Equation/Align、MathTree/Flow/Node | 父链、slot、模板样式、MathTree 导航/布局/源码 adapter、节点/schema 校验和历史映射均消费身份与结构；已知 DTO 与顺序字段不能仅因单一路径未读而删除。 | 保留；未知数据和 schema 版本按兼容契约处理。 |
| `CdocFile.media` / `Media` | 打开校验唯一非空 ID、非空类型、标准 Base64 和 32 MiB 解码上限；Session 保存快照携带媒体，测试验证未知元数据往返。图片解码/尺寸校验属于 task 052。 | 保留。 |
| Inline/Block `definitionId` | 两类均序列化；Inline 参与格式/数学模板语义，Block 当前仅持久化并推导 capability。打开 `.cdoc` 仅在能力和主题版本可完整校验时调用完整文档验证，否则进入安全预览；直接 `Document::validate()` 始终完整验证。 | 保留，等待 Block 模板语义设计。 |
| 节点 ID、Opaque/`extra`、格式/主题/MathTree schema version | 唯一性、物理定位、编辑/历史身份、未知内容往返、安全拒绝有损变换及版本兼容均依赖这些字段。 | 保留；不为统一位置 key 给 Opaque/ExplicitBreak 增加 ID。 |

Session 位置与生命周期审计未发现可安全删除的重复字段：逻辑 `Pos` 与 anchor/focus `VisualAffinity` 分别表示文档坐标和排版候选；`session_id`/`document_generation` 绑定快照，源码草稿另校验 draft revision/projection identity；`math_edit` 与 `host_caret_position` 分别承载数学分支和共享宿主三态；单个选中环境、文档序结构选区及其 anchor/focus 各有删除/扩展语义；`paragraph_index`、大纲 `structure_index` 与 LRU `text_map_cache` 是不同派生视图；Undo/Redo 的 state ID、下一个 ID、估算字节和 merge epoch 各用于保存点身份、分配、容量与分组。`PositionMap::map` 是事务生成的纯坐标变换，不携带来源 Session/generation，也不替代结果位置验证；跨文档复用不受支持。

## 验收标准

- [x] 嵌套块、标题、空容器、连续公式块、InlineMath 和 Paragraph 文本之间的所有可证明位置有一致顺序；稳定 ID 和 ordinal 不充当排序键。歧义边界、跨源/树次序冲突和跨代位置明确拒绝。
- [x] 活动结构父链可按有效宿主 ID 查询；不从任取 Paragraph 推断 Block 状态。
- [x] 只读与可变 locator 按当前块树解析唯一父环境 ID 路径；目标节点、slot、Session 与文档代际由调用方校验。
- [x] Session/历史位置字段有完整消费者和生命周期证据；可合并的重复状态已收敛。
- [x] 所有候选模型字段均完成消费者/往返审计；保留的 wire 字段有兼容理由，未发现可安全删除项。
- [x] Undo/Redo、删除、拆分、合并及跨容器移动后的映射有效；失败时原子拒绝且保留文档/Session 状态。
- [x] ExplicitBreak/Opaque 的物理 slot 与无稳定 ID 语义已核对，不为排序添加持久 ID。
- [x] 代表性性能基线完成；当前只测一组深层父链/宽根序列负载，不代表所有载体或规模；无前后对照时不宣称性能提升。
- [x] 最终相关测试、lint、格式和独立评审通过；全 workspace 验证已在本轮代码改动结束后重跑。

## 验证记录

- **2026-11-11 当前分支验证：** `cargo test --workspace --locked` 全部通过（Engine 752、App 285、Doc 78、Structure 13、native/QML 聚合 11 及其他 crate/doc tests；1 项受控 crash 子进程测试 ignored）；执行期间还通过 `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check` 与 `git diff HEAD --check`。Criterion 基线命令和结果见[Rust 性能范围](../../tests/performance/rust/README.md)。

## 独立评审

- Terra 领域审查覆盖位置比较、载体映射、宿主路径及 Session/历史生命周期；未发现阻断问题。TreeBoundary 无法唯一映射到相邻 TextFlow 两侧时继续拒绝，避免制造顺序。
- Sol 对最终完整 diff 做只读复核，先指出基准计时范围和 locator 验收描述两项问题；修正后确认无剩余 findings。基准目标现位于 512 个根层段落之后，验收明确由调用方校验目标节点、slot、Session 和文档代际。

## 风险与回退

错误顺序会影响焦点、Shift 范围和删除落点；陈旧 locator 会指向已移动或已删除节点。任何不能验证唯一映射的坐标都必须拒绝。共享父链查询只解析当前物理路径，不代替 ParagraphLocation 的 Story、文本偏移、document generation、目标 slot 和事务快照校验。
