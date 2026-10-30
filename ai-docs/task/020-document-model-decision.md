# 020 — Paragraph/Run 语义与文档模型细化

> 本文件是任务 020 的状态和验收索引。候选比较见[分析记录](020/analysis.md)，评审及用户决策历史见[历史记录](020/history.md)。当前规则以[文档模型索引](../architecture/editor/document-model.md)所列专题规范为准。

- 状态：done
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-30

## 目标与背景

产品已决定使用 Paragraph/Run 表示文字，并使用可嵌套 EnvironmentNode 表达内容结构和格式作用域。本任务细化 EnvironmentNode 的行内/块级 scope、允许的子节点、背景和删除语义。Paragraph/Run 本身不承担块级层次、自动编号、公式/表格对象和整组删除语义。此决策影响命令、选择、分页、缓存和文件格式，先于实现。

## 必读

- [文档模型](../architecture/editor/document-model.md)、[运行时](../architecture/core/runtime.md)、[命令](../architecture/editor/commands.md)
- [分层](../standards/layering.md)、[Rust](../standards/rust.md)
- [WordCraft 参考笔记](../research/004-wordcraft-editor-design-notes.md)

## 范围与非目标

比较并细化段落/Run 富文本树、语义块树加段内 Paragraph/Run、以编辑操作/协作日志为核心的表示。结合用户已选 Paragraph/Run 基础表示，确定语义容器如何组合，而不是重新悬置是否使用 Run。对比结构表达力、编辑复杂度、Unicode 位置、撤销、公式/表格、分页、文件格式演化和缓存。

比较模型和位置方案时，也要评估长文档下的编辑、选区查询、索引更新和内存成本；标出关键操作的复杂度与潜在热点。避免每次小改都复制整棵文档树或扫描无关内容。没有基准前不承诺具体耗时，相关热点由 013/027 的基准与验证补证。

定稿 EnvironmentNode 的 scope（inline/block）、kind、允许的子节点、粗体/斜体嵌套与继承、块级标题/编号、子树删除/提升策略，并区分 PageSection（页面属性）。给出位置契约、其余样式继承、段落标记、块/内联对象边界、稳定 ID 与 rev 使用规则；明确哪些是文档内容、哪些是派生布局。记录版权的一般原则并区分许可证、专利、商业秘密和商标。当前环境高亮行为已定，规范见[结构导航](../architecture/editor/structure-navigation.md)。实现与规格独立编写，不复制 WordCraft 的代码、文档表达或测试。

不包含：实现 crate、采用 docx 为原生格式、协作 CRDT 或 Word 文件兼容承诺。此任务不是法律意见或专利自由实施检索。

## 前置条件与待决策

Paragraph/Run 和嵌套 EnvironmentNode 表示已经选定；块级环境标题采用独立 `title: Option<Paragraph>`；M0 一个 PageSection 覆盖整份正文。多节扩展和生产 `.cdoc` 序列化细节仍待实现任务验证；完成本任务前不得据其定型生产文件格式。

## 候选模型分析

候选模型比较、共同场景和复杂度假设见[分析记录](020/analysis.md)。该记录用于解释当时的选型过程；当前数据和行为契约以架构专题规范为准。

## 实施步骤

1. 用普通文本、格式混合选区、嵌套粗斜体、嵌套块环境、空段、表格单元格、公式、显式分页和自动分页作为共同场景。
2. 让每种候选表示逐项走查这些场景，说明数据、命令及失败/撤销边界。
3. 记录版权边界和代码复用规则，不声称完成专利清查。
4. 在既定 Paragraph/Run 表示上定稿语义结构组合，并更新文档模型和运行时边界。
5. 将实现拆成 021–027 并独立评审。

## 预计改动

更新编辑器架构文档和任务索引；不改生产代码。

## 验收标准

- [x] 每个方案覆盖普通文本、格式、空段、表格、公式、显式分页和自动分页。
- [x] Paragraph/Run 保存文字；EnvironmentNode 同时支持 inline 粗斜体与可嵌套 block 环境，PageSection 单独保存页面属性。
- [x] M0 恰有一个 PageSection，作用于整份正文；多节范围和分节节点不进入首版格式。
- [x] `.cdoc` 用顶层 `requiredFeatures` 声明编辑所需能力；未知项允许只读打开、拒绝写入。未知可选字段往返保留；能力标识唯一登记在[`.cdoc` 存储规范](../architecture/editor/document-model/document-storage.md)中。
- [x] EnvironmentNode kind 仅从内置类型注册表选择；每种类型明确 scope、合法子节点和语义。文档主题可替换每种 kind 的默认环境模板并定义同 kind 变体；实例可选 `definitionId`，其 `baseKind` 必须匹配，实例 `props` 作局部覆盖。模板可组合既有格式环境和样式值，但不展开成文档子树，也不能改写结构语义。未知 kind 只读打开，首版 kind 见[环境模型](../architecture/editor/document-model/document-environments.md)，feature 标识见[`.cdoc` 存储规范](../architecture/editor/document-model/document-storage.md)。
- [x] 首版环境模板使用声明式数据，可组合内置格式环境、命名样式和覆盖值；不执行用户脚本或通用宏语言。
- [x] 首版 registry 包含行内 Bold/Italic/Font、块级 Section/Custom；后续类型必须扩展该注册表和能力定义。
- [x] Font EnvironmentNode 保存局部字体族；Run 不直接保存字体族；未由环境、文档全局覆盖或主题指定时才使用系统默认文本字体。
- [x] Font 环境按字体族覆盖嵌套；同族相邻/冗余范围合并或压平，不同族嵌套保留。
- [x] 格式完整优先级为 Run/Paragraph 局部显式属性 → EnvironmentNode 有效设置（实例覆盖模板，内层向外继承）→ 文档全局覆盖 → 主题命名样式 → 应用默认值。
- [x] 文档级全局设置选择文档排版主题和默认 Run/Paragraph 样式，并可设置全局格式覆盖；`.cdoc` 保存主题当前格式快照并可记录来源 ID。运行时只支持一个主题格式，加载旧主题/快照时迁移到当前结构，不并行保留旧版本逻辑。
- [x] 新建文档的 `defaultFontFamily` 未设置；未由环境、全局覆盖或主题指定时才使用系统默认文本字体。默认字体命令才写入显式全局覆盖。
- [x] 旧主题/快照载入时迁移到当前结构；外部主题源文件只在用户显式保存时写回，已有文档只在显式更新时替换快照。
- [x] 粗体和斜体只有行内 EnvironmentNode 一种持久化表示，Run 不保存这两种直接覆盖。
- [x] 粗体与斜体可互相嵌套；`enabled: true|false` 分别显式开启/关闭，缺少节点表示继承，同状态且元数据相同的同类环境才压平；相邻范围仅在兼容时合并，其他语义环境不自动合并。若序列化显式状态，`.cdoc` 声明 `caret.env.explicit-format-state`。拆分时原 ID 留左段、合并保留靠前 ID，撤销恢复原节点和 ID。
- [x] 样式变更按布局度量、纯绘制、大纲语义和来源元数据分类并只使相应缓存失效；纯字色不重排也不重建结构索引。当前架构按布局与结构职责分别失效；结构事务提交后从文档树重建索引。当前 DTO 使用拥有型 `Vec`，不承诺局部编辑只复制受影响节点；更细粒度缓存或结构共享需由基准支持，见[文档结构](../architecture/editor/document-model/document-structure.md)与[结构导航](../architecture/editor/structure-navigation.md)。
- [x] 当前环境高亮遵循[结构导航](../architecture/editor/structure-navigation.md)的唯一行为定义。
- [x] Section 持有编号定义；顶层按文档顺序编号，嵌套 Section 依据 Section 祖先生成层级编号，只拼接启用编号的祖先，普通容器不打断计数。模板提供默认显示开关与逐层格式，实例可覆盖；格式至少支持阿拉伯数字、小写/大写罗马数字和中文数字。关闭编号时不生成标签且不占用同级序号。模板不能改变层级/计数关系。编号是标题前的虚拟标签，不是持久化 Run、不计入文本偏移；解除时物化为普通文字。块环境整体删除移除标题和整棵子树；解除环境保留标题并将其转成首个普通 Paragraph，再按原顺序提升正文子项，保留子节点和标题 Paragraph 的 ID。
- [x] 文档内容/派生布局、UTF-8 偏移/字素移动、稳定 ID/缓存 rev 边界明确。
- [x] 文档默认语言、Paragraph 覆盖、Run 局部覆盖按 BCP 47 标签解析；应用界面语言独立，自动检测不静默修改正文或持久化语言标签。
- [x] Run 在插入、加载和保存时保留原始 Unicode 码点序列；需要规范化搜索/比较时另算视图，不回写正文。
- [x] Rust 位置以 UTF-8 字节偏移为准，键盘移动按同一 Unicode 版本的 extended grapheme cluster；Qt/C++ UTF-16 只在 FFI 边界转换。
- [x] 行内环境边界上的 Pos 携带 upstream/downstream 亲和性，并决定插入文本继承的环境链；环境两侧呈现“无环境｜有环境｜无环境”。点击按命中的字素侧选 affinity，歧义精确边界选 downstream；简单 LTR 边界的方向键切换表见[文档位置](../architecture/editor/document-model/document-position.md)，环境编辑规则见[环境模型](../architecture/editor/document-model/document-environments.md)。
- [x] 位置块路径使用有类型的 `EnvironmentBody`、`TableCell`、`Paragraph` 和 `EnvironmentTitle` 段，均使用稳定 ID、不使用数组下标；解析时验证段类型和包含关系。节点移动保留 ID，删除/替换由事务提供位置映射。
- [x] 块级 EnvironmentNode 的独立标题 Paragraph 可通过稳定节点 ID 定位，且不属于 `children`。
- [x] 块环境标题保持单段；标题中 Enter 将后半段移入新建的正文首段并插到现有内容前，正文为空或后缀为空时也创建对应 Paragraph。
- [x] 一般版权原则有辖区边界；没有绝对无侵权保证。
- [x] 021–027 的任务依赖无环，各任务均有独立目标和验收标准。
- [x] 属性归属按目标 kind 白名单校验；粗体/斜体可由主题/环境继承，同时有且仅有 EnvironmentNode 能持久表达显式关闭，不恢复 Run 粗斜体属性；`enabled: true|false` 分别显式开启/关闭。
- [x] 相交格式区间用字素边界切分并树化；新范围置于已有环境链的内侧以保留已有模板优先级；单次同时创建多种环境时按 Bold → Italic → Font 嵌套，不跨命令重排已有祖先。树形可依命令顺序不同，但各自语义确定；同值冗余判定、definition/props 合并条件和 Undo ID 恢复明确。
- [x] 同一 Paragraph 的字素分段跨 Run 与行内格式环境；样式边界不制造停靠点，行内对象是原子边界；交互选区向外吸附到完整字素，非法 API 偏移明确拒绝。
- [x] 逻辑边界亲和性与视觉 caret affinity 分离；权威 Pos 以 story ID、终止稳定节点 ID、UTF-8 偏移为锚，类型路径可重建；跨父移动、拆分、合并、解除和删除均有位置映射。
- [x] Section 计数流按最近已编号祖先和显示层级共享，Custom/未编号 Section 对标签和计数透明；无标题 Section 不显示标签、不占号但对后代透明；批量解除先快照编号再原子改树。标题 Enter 覆盖标题首尾、标题内选区、跨标题/正文选区和 IME composition 边界。
- [x] 未知必需能力与主题迁移有安全预览边界；未知内容不被静默遗漏，未来主题版本/缺少迁移器不猜测，迁移先在副本验证再安装。
- [x] 撤销/重做恢复文档后重建派生结构索引；当前 DTO 没有逐节点 `contentRev`、`layoutRev`、`structureRev` 或 `paintRev`，也不以 `Arc` 共享推断性能。代表性长段落、宽容器和 Section 编号基准由后续布局/性能任务维护。
- [x] 独立评审通过；最终复审未发现未处理的 P1/P2。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | Paragraph/Run、语义块树和操作日志共同场景矩阵 | 覆盖结构、编辑、Unicode、长文档性能与布局约束 | 完成；每类候选按相同场景比较 |
| 2026-10-25 | 独立模型评审 | 决策可实现、法律表述有边界 | 已完成；确认核心架构可保留，但发现显式格式关闭、区间树化、逻辑/视觉位置、编号域、安全预览、迁移版本、修订令牌及复杂度等待定项；020 未通过验收 |
| 2026-10-25 | 中间独立复审 | 检查首轮缺口修订及相关实现验收 | 发现两项 P2，已修复：保留已有模板/祖先优先级；Section 祖先前缀变化时刷新后代标签与标题布局。另补未知可选字段在节点变换中的保真验收。 |
| 2026-10-25 | Markdown 本地链接、任务依赖图、`git diff --check` | 无断链、无循环、无空白错误 | 89 个维护中的 Markdown 文件中的 377 个本地链接目标/锚点有效；28 项任务依赖无缺项、无环；完整 diff 检查通过 |
| 2026-10-25 | 最终独立评审 | 完整审阅 020 及相关架构/任务 diff | 通过；检查相对 HEAD 的完整 diff 及相关架构/任务契约，未发现未处理的 P1/P2；领域和性能测试留给实现任务 |

## 风险与回退

过早固定表示会把复杂性推到命令与迁移层。如果场景不能区分候选方案，补纸面原型；不提前创建空 crate。

## 评审与决策记录

独立评审发现、定稿边界示例和逐项用户决策保存在[历史记录](020/history.md)。

## 完成摘要

Paragraph/Run 与语义块树的边界、环境注册表和模板能力、样式优先级、格式范围树化、Unicode 字素和位置映射、Section 计数与标题编辑、安全预览及主题迁移规则均已定稿并同步到架构规范。中间复审发现的问题已记录处理，最终独立评审通过。运行时索引策略和性能结论以当前架构文档及后续实现任务证据为准；本任务记录的是设计决策，不表示每节点修订或局部共享已实现。
