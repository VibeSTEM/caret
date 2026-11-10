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

## 模型字段审计记录

| 字段 | 运行时消费者 / 持久契约 | 决定 |
| --- | --- | --- |
| `DocumentSettings.defaultStyles`、`formatOverrides` | 当前没有样式解析器读取这两项；它们仍是 `.cdoc` 的已知 JSON 字段，文档结构图列明，存储往返测试和 App 保存重开测试验证保留。类型化 map 要求 JSON 对象，缺失值反序列化为空 map 并按默认序列化输出空对象。 | 暂留为显式字段；改放 `extra` 会改变缺省序列化和无效类型的接受行为，即使保留非空对象的 wire 内容也不等价。 |
| `ThemeSnapshot.namedStyles` | 当前无排版/编辑器运行时读取；主题快照 DTO 将其定义为已知字段，存储与 App 保存重开测试验证往返。类型化 map 要求 JSON 对象，缺失值采用默认空 map。 | 暂留为主题快照已知字段；改放 `extra` 会放宽无效类型接受行为，且其主题格式/未来消费者尚未完成审计。 |
| 顶层 `CdocFile.media` 与 `Media` | 打开/保存校验 ID 非空且唯一、`mediaType` 非空、`data` 为标准 Base64 且解码不超过 32 MiB；`DocumentSession` 保存快照携带媒体数组；task 052 规定图片内容后续使用该资源容器；存储与 App 测试验证媒体和未知元数据往返。当前没有图片解码、格式识别或像素尺寸校验。 | 保留；已有持久化、输入验证和会话消费者，图片语义校验留在 052 范围。 |
| Paragraph `language` / `layout` / `markStyle` / `props` / `extra` | language 参与 shaping；layout 进入排版与分页；markStyle 参与段落合并兼容性判断，空行需要 mark metrics 且存在非空 markStyle 时明确拒绝当前不支持的样式；props 的 `outlineLevel` 被结构索引消费；extra 在序列化中保留，并在不支持无损变换时阻止编辑。 | 保留；运行时、校验、未知字段兼容及事务保护均有实际消费者。 |
| 环境 `definitionId` | 两类 ID 均经 codec 读写；能力收集器为 Block 推导 `caret.env.templates`，为 Inline 另推导 `caret.env.inline-text-templates.v1`。`.cdoc` 打开路径仅在所需能力均已知、Inline 能力已声明且主题版本有效时调用完整 `Document::validate()` 执行模板语义校验，否则走安全预览校验；直接调用 `Document::validate()` 始终执行完整校验。Block 暂无模板解析器，也无同等的模板引用/能力声明强制校验。行内格式/数学转换保留 variant ID，历史内存估算计入 `definitionId`。 | 保留；明确区分已实现的行内语义与仅持久化/能力推导的块环境字段，后者待块模板设计再赋予运行时解释。 |
| 节点 ID、`Opaque`、`extra`、格式/主题/MathTree schema version | 节点 ID 参与文档唯一性验证、物理索引、编辑与历史；Opaque/extra 用于未知内容往返及安全拒绝有损编辑。外层文档版本不兼容时拒绝打开；主题快照未知版本进入安全预览；旧 MathTree schema 拒绝，未来 schema 保持 Opaque 并按安全预览规则处理。 | 保留；属于身份、前向兼容和版本解释边界，不是无消费者的缓存字段。 |

## 验收标准

- [ ] 有效文档位置在嵌套块、标题、空容器、连续公式块、InlineMath 与 Paragraph 文本之间可确定顺序；稳定 ID 只作身份，Paragraph ordinal 不充当全树排序键。
- [x] 按任意有效宿主 ID 查询到正确父链和同级次序；活动结构状态不从任取 Paragraph 推断。
- [ ] locator 统一共享身份、父链、排序与 generation 校验；只读查询与可变事务定位都使用正确且经验证的入口。
- [ ] 位置/Session/历史字段审计有消费者和生命周期证据；冗余状态得到收敛，必要的扩展与兼容字段保留理由明确。
- [ ] 持久模型字段逐项有序列化与领域消费者或兼容契约依据；拟删除字段已验证所有构造/读写/round-trip/兼容路径，未删除字段记录其扩展或未知数据契约。
- [x] 已审计 `defaultStyles`、`formatOverrides`、`namedStyles` 和顶层媒体候选字段；明确标记无运行时解释的已知 wire 字段并保留，确认媒体资源存在校验、会话快照和往返消费者。
- [ ] Undo/Redo、删除、拆分、合并和跨容器移动后的映射保持有效；无法映射时原子拒绝并保留文档与会话状态。
- [x] 不因排序实现需要就为 ExplicitBreak/Opaque 添加 schema ID；若新增身份有明确需求和往返/兼容/历史证据，记录独立决策。
- [ ] 性能目标有代表性文档与可复现基线；未测量时不宣称复杂度改进。
- [ ] 相关格式/lint、行为测试和独立评审通过；未执行验证明确标记。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-10 | `cargo test -p caret-structure --locked` | StructureIndex 导航语义与物理父链/slot 顺序回归通过 | 10 项通过；物理节点及嵌套内容外边界可比较，但尚未覆盖任意 Paragraph caret 与 InlineMath 局部位置的全序 |
| 2026-11-10 | `cargo test -p caret-engine --locked` | locator、撤销重做与失效映射回归通过 | 710 项通过；复合历史补丁部分失配及同 ID 内容变化时原子拒绝 |
| 2026-11-10 | 段落位置比较的 Story、locator 与 TextMap 边界回归 | 无效位置不参与排序，段落内 text/InlineMath 占位偏移保持真实顺序 | wrong Story、越界 offset 和失效 block path 定向测试通过；通用宿主 caret 全序仍未实现 |
| 2026-11-10 | `cargo test -p caret-engine --locked`、`cargo test -p caret-structure --locked` 与两 crate Clippy | 校验后的段落、物理边界及数学局部排序通过 | 729 项 engine、10 项 structure 测试及 Clippy 通过；覆盖 InlineMath 占位符、标题/IME、嵌套 MathTree、跨 TextFlow 组合字素、草稿修订、失效位置及同宿主比较 |
| 2026-11-10 | `cargo test -p caret-engine session_caret_comparison --locked`、同一数学文本岛路径顺序定向测试 | 文本亲和性、空段落与宿主边界排序保持一致；Section 正文焦点及 InlineMath/InlineEnvironment 能映射到段落边界 | 16 项比较器测试和 1 项不同 TextFlow 路径测试通过；覆盖空段落中前后 Equation 边界传递性、空 Inline 外侧与两种 affinity、相邻 InlineMath 边界的 affinity 顺序、嵌套 Inline 同侧边界的歧义拒绝、InlineMath 与文本位置、Section 正文的真实 Session 焦点、Equation 与文本/相邻 Block/InlineMath/Section 边界的顺序、Align 行列顺序、真实当前 Align 焦点快照，以及同一连续公式文本岛内不同 TextFlow 的顺序与共享边界 affinity。完整 workspace 验证待本轮实现收敛后重跑 |
| 2026-11-10 | `cargo test -p caret-engine session_caret_comparison --locked`、跨 TextFlow 路径及空 TextFlow 定向测试 | 同一 MathTree 内 TreeBoundary 与 TextFlow 位置、不同文本路径和空文本流保持可证明且传递的顺序 | 18 项比较器测试及 2 项路径测试通过；覆盖不同 TextFlow 顺序、TreeBoundary 与文本流起止/内部位置、空 TextFlow 两种 affinity 与前后 TreeBoundary 的传递顺序。 |
| 2026-11-10 | `cargo test -p caret-engine history_parent_locator_uses_physical_parent_ids_and_rejects_moved_paragraphs --locked` | Paragraph 历史 locator 从物理节点定位复用真实父链，并拒绝移动后的陈旧父链 | 1 项定向测试通过，覆盖标题、嵌套正文、顶层正文 slot 与移动后拒绝。 |
| 2026-11-10 | `cargo test -p caret-engine editing::session::history::tests --locked` | 历史段落 locator 与 Undo/Redo 对标题所属环境、同父移动陈旧路径及原子恢复保持正确 | history 模块 7 项测试通过；新增覆盖嵌套 Section 标题 `DocumentPatch` Undo/Redo、标题 owner 父链、同父 slot 移动后旧 ParagraphIndex 拒绝、索引键与 Paragraph ID 不匹配拒绝，以及跨父移动后的物理父链校验。 |
| 2026-11-10 | `cargo test -p caret-engine --locked`、`cargo clippy --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff HEAD --check` | 验证 Paragraph locator 从 history 移到 paragraph_ops 后，历史、结构和数学编辑消费者仍通过共享验证入口 | engine 746 项测试、caret-engine Clippy、格式与差异检查通过；全 workspace App/native/QML 测试待 082 收敛后验证。 |
| 2026-11-10 | `cargo test -p caret-engine --locked`、`cargo clippy --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff HEAD --check` | 验证 InlineMath 内部位置与其他宿主外边界之间可证明的物理次序 | engine 747 项测试、caret-engine Clippy、格式与差异检查通过；新增覆盖相邻空 Inline、嵌套 Inline 与 Section 边界的双向比较、相邻公式与 affinity 次序，以及越界 MathTree 坐标/宿主错配拒绝。 |
| 2026-11-10 | `cargo test -p caret-engine session_caret_comparison --locked`、跨岛文本位置定向测试 | 验证同一 MathFlow 序列中被结构项隔开的 MathTextFlow 位置可按真实内容次序比较，且无效文本边界拒绝 | 22 项 Session 比较器测试通过；新增覆盖分隔符两侧双向次序、affinity 不改变跨岛结构次序、空 TextFlow、SoftBreak、UTF-8 字素内部偏移和超界偏移拒绝、不同操作数拒绝及 TreeBoundary 传递性。 |
| 2026-11-10 | `cargo test -p caret-engine --locked`、`cargo clippy --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 验证同序列跨文本岛排序增量与整个 caret-engine crate | 750 项 engine 测试、caret-engine Clippy、格式与差异检查通过；全 workspace App/native/QML 测试待 082 收敛后验证。 |
| 2026-11-10 | `cargo test -p caret-engine session_caret_comparison --locked`、`cargo test -p caret-engine --locked`、caret-engine Clippy、格式与差异检查 | 验证 MathTree 嵌套操作数文本岛与结构边界使用同一树顺序键 | 22 项 Session 比较器测试及 750 项 engine 测试通过；覆盖分数上下操作数按 children 次序、其与根 TreeBoundary 的跨层传递，以及此前的空流和 affinity 用例。 |
| 2026-11-10 | `cargo test -p caret-engine session_caret_comparison_uses_parsed_fraction_operand_order --locked` | 核验解析器实际生成的分数子流与树顺序键一致 | 解析 `\frac{\text{n}}{\text{d}}` 并从生成的嵌套路径定位分子/分母 TextFlow，比较结果符合 MathTree 子项顺序。 |
| 2026-11-10 | `cargo test --workspace --locked` | 全 workspace Rust、文档集成及 native/QML 测试通过 | 737 项 engine、10 项 structure 测试及其他 workspace/native/doc 测试通过；1 项注明的受控 crash 子进程测试保持 ignored。 |
| 2026-11-10 | `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 全 workspace lint、格式与差异检查通过 | 全部通过。 |
| 2026-11-10 | `cargo test -p caret-engine --locked`、`cargo test -p caret-structure --locked`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、格式与差异检查 | 验证新增跨宿主块数学位置与 Align 行列顺序，不回归 locator/结构逻辑 | engine 739 项、structure 10 项通过；workspace Clippy、`cargo fmt --all -- --check`、`git diff --check` 通过。此后新增同一公式文本岛跨 TextFlow 路径比较，验证待本轮收敛后重跑。尚未重跑 workspace App/native/QML 测试。 |
| 2026-11-10 | `cargo test --workspace --locked`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 验证跨宿主、MathTree/TextFlow、空 TextFlow 比较和全 workspace 集成回归 | workspace 测试通过：engine 744 项、structure 10 项、App 285 项、native/QML 集成 11 项，以及其余 crate、文档和 doc tests；1 项受控 crash 子进程保持 ignored。workspace Clippy、格式与差异检查通过。 |
| 2026-11-10 | `cargo test -p caret-app -p caret-doc --locked` | 文档编辑与格式/未知数据往返通过 | 285 项 App、78 项 Doc 通过；包含打开、编辑、保存、重开后保留样式映射、主题命名样式和媒体 |
| — | 代表性文档定位基准 | 提供性能基线，不先定复杂度结论 | 未执行；不宣称性能改进 |
| 2026-11-10 | `cargo format --check`、`cargo quality` | 格式、lint、依赖审计与全 workspace 测试通过 | 通过；cargo quality 在沙盒外运行以访问只读保护的 cargo advisory 数据库 |
| 2026-11-10 | Terra 位置比较器领域复审 | 宿主身份、坐标范围、字素边界与顺序等价保持一致 | 复核发现 Section 内 Paragraph 焦点被拒绝，以及空段落可能令块边界“相等”不传递；已修复验证和 tie-break，并增加 Section、空段落、Inline 边界测试。第二轮只读复核与完整 final review 待完成 |

## 风险与回退

错误文档顺序会影响焦点、Shift 范围和删除落点；错误 locator 会指向已移动或已删除节点。任何无法校验的路径都必须拒绝，不回退到任意 Paragraph 或按 ID 字典序猜测。

## 决策与工作记录

- 2026-11-09：从 080 拆分 Issue #126 的 H1、M1、M3、M4 相关工作；不照搬固定 enum、直接改 `StructureIndex` 为可变索引、给 Opaque/ExplicitBreak 统一加 ID 或未测 O(深度) 承诺。
- 2026-11-10：实现前审查确认 `StructureIndex` 的 heading 父链表示大纲语义，不是文档树物理父链；新增/复用定位契约时必须保留两者的区别。物理路径保留每一层真实同级 slot，包括没有稳定 ID 的 ExplicitBreak/Opaque；这些节点不得因此获得持久 ID 或编辑能力。
- 2026-11-10：物理定位入口落在 `caret-structure`，保留 `StructureIndex` 的大纲语义；公式块路径和活动环境父链改用宿主 ID 查询，Paragraph 缓存与历史路径核对完整父环境链及真实 slot。无 ID 的显式分页和 Opaque 仍只占 slot，不创建持久身份。
- 2026-11-10：历史补丁重放在候选文档上事务应用；复合补丁中任何一项路径、slot 或快照不匹配时拒绝整个撤销/重做，不推进 Session 历史游标。Paragraph 快照删除要求 ID 与完整值同时匹配。
- 2026-11-10：段落位置比较现校验当前文档上的目标 ID、Story、完整物理路径及 TextMap 合法边界后才排序；Story 错配、越界偏移和陈旧路径均拒绝。此比较仅覆盖 Paragraph 文本位置，不能代表 Inline/Block 宿主状态或数学局部位置的通用全序。
- 2026-11-10：结构层新增同一文档快照内的节点外前/外后比较；同一有序序列中相邻节点共享间隙，无 ID 节点仍占 slot，父块外侧包围标题和后代。该入口只比较宿主物理外边界，不替代 Paragraph、Inline 或 Math adapter 的宿主内部坐标比较。
- 2026-11-10：MathTree adapter 可比较同一宿主标签下的结构序列位置，路径及局部 offset 必须都能解析；调用方仍负责证明 host、目标树及文档代际一致。该排序不等价于源码偏移或 TextFlow 路径顺序。
- 2026-11-10：同一 TextFlow 路径的局部位置先映射到连续数学文本岛，再校验组合字素边界；相邻 TextFlow 或 InlineEnvironment 两侧的局部边界若落在同一字素簇内部则拒绝。该局部比较不负责排序不同 TextFlow 路径。
- 2026-11-10：数学宿主位置投影保留 TextFlow/MathTree 的逻辑 affinity；源码位置绑定其已提交投影 ID，未解析草稿则绑定 Session 内草稿 ID 与内容修订；所有快照仍须校验 Session 与文档 generation。
- 2026-11-10：新增临时 `SessionCaretSnapshot` 包装当前文本或宿主三态位置，并校验 Session/document generation 与源码草稿修订；它不替代宿主路径、Story 或局部坐标 adapter 的验证。
- 2026-11-10：新增 Session 级只读比较入口，已接入段落位置比较、块外前/外后物理边界、Section 子树中的 Paragraph 位置、InlineEnvironment 段落位置与边界、InlineMath 内部/外侧与 TextMap 边界，以及同一数学宿主中的 MathTree、TextFlow 和源码位置。文本同偏移按 `Upstream < Downstream` 排序；可比较的宿主起点/终点分别与起点 Upstream、终点 Downstream 规范为同一顺序位置。相邻 Inline 外侧边界在同一物理间隙时按相应 affinity 次序区分；嵌套宿主同偏移的多个外侧边界不强行合并到一个文本位置；空段落和空 Inline 保留左右边界，不折叠到同一个位置。源码比较严格使用 Align 可编辑区间和字素边界；MathTree 路径拒绝不可达的可选占位槽。错误宿主类型、目标 ID、Story、节点路径、局部范围和修订身份均拒绝。
- 2026-11-10：比较器按宿主/坐标适配拆为 Session 分派、Inline span resolver 和数学 adapter；affinity 次序复用位置模块唯一实现，Align 的 target/host 与 row/cell identity 解析统一供结构命中、文本岛边界导航和比较器使用；源码 draft/projection 版本匹配由单一 helper 负责，局部位置 adapter 继续校验 body 范围和字素边界。移除仓内仅测试调用的节点起点公开比较 API，测试直接覆盖外侧边界查询。定向测试放入对应 `document_position/tests.rs`，不继续扩大会话杂项测试文件。
- 2026-11-10：Session 全序比较新增 Equation/Align 内部位置与 Paragraph 文本、其他数学宿主及块外边界的物理顺序映射；Align 内部跨行/列位置按文档实际 cell 序号排序，同一单元格继续使用数学局部 adapter。映射依赖当前已校验的节点外边界，不用稳定 ID 推测顺序；相交或无法证明唯一顺序的位置仍拒绝比较。定向覆盖真实 `current_caret_snapshot` 产生的不同 Align 行焦点。
- 2026-11-10：数学宿主内部位置与不同宿主的外前/外后边界复用物理区间比较；Equation/Align 及 InlineMath 内部 caret 都可与其外部相邻或包围宿主排序，Inside 严格位于自身 Before/After 之间，无法证明区间先后的情况仍拒绝。相邻 Inline 外侧边界的既有 affinity 次序保持独立，不由数学宿主比较重写。
- 2026-11-10：同一 MathFlow 序列中被结构项隔开的 TextFlow 位置可按各自文本索引排序；先分别验证路径、UTF-8/字素边界，再比较所属序列和内容索引。不同嵌套序列、不同操作数及无法证明同一序列的位置仍拒绝，不从路径编码或 affinity 推断跨分支顺序。
- 2026-11-10：跨不同数学文本岛的位置共用 MathTree 序列顺序键；操作数与环境子流按模型有序子项次序，TextFlow 首尾映射到对应结构边界，内部位置位于该内容项的严格区间，空 TextFlow 的 affinity 决定落在前/后边界。相同文本岛仍优先使用展平后的字素顺序，以保留相邻 TextFlow 共享边界的既有亲和语义；不同宿主、树代际或无法验证的局部坐标仍拒绝。
- 2026-11-11：MathTree 操作数顺序代表语义槽位顺序，和水平树导航及 TreeBoundary 比较保持一致；不能据此推断所有节点的 LaTeX 源码顺序。源码映射明确允许部分节点的语法范围顺序不同于 children 槽位，因此 SourceUtf8 与 Tree/Text 坐标混合时继续拒绝，直到存在能验证同代投影、唯一映射及两种顺序关系的 adapter。
- 2026-11-10：同一连续数学文本岛中的跨 TextFlow 比较复用文本岛展平结果、全局字节偏移和字素边界校验；数学结构边界与文本流按 TextFlow 的结构起止边界映射比较。若同一结构边界对应多个 TextFlow 端点，且待比较 TreeBoundary 正好位于该边界，则拒绝歧义映射，避免比较相等关系不传递；远离该边界的位置仍按结构顺序比较。空 TextFlow 的 Upstream/Downstream 分别归属其前后边界。MathTree 与文本路径必须解析到相同目标树和 Align cell。
- 2026-11-10：Paragraph locator 的共享校验入口归入 `editing::paragraph_ops`：同时校验 ParagraphIndex 缓存路径、ID、标题/正文类型、Story 和完整稳定父链，并返回段落与当前父链/slot。历史捕获直接复用该结果；结构操作与数学编辑经 `paragraph_parent_path` 复用同一校验，不再从 history 模块获取定位职责。标题父链保留所属环境 ID，供历史恢复定位 owner；ParagraphIndex 仍负责历史快照定位。当前先按缓存路径校验段落，再按 ID 查询物理 locator；没有性能基准，不宣称降低遍历成本。
- 2026-11-10：Session 比较器仍是明确受限的可比较子集，不代表完整跨宿主全序。同一 MathTree 的 TreeBoundary/TextFlow 与不同嵌套数学序列的文本位置现使用统一结构顺序；同一文本岛的 TextFlow 位置保留展平字素顺序。Source 与树/文本坐标混合、复杂 Block 父子内部坐标尚无统一 adapter。不同数学宿主仅在物理节点范围能证明不相交时可排序，重叠或映射含糊时拒绝。082 保持 in-progress，继续处理跨宿主组合和剩余字段/消费者审计。

## 完成摘要

未完成。
