# 数学公式

更新日期：2026-11-10。本文是 Caret 数学文档模型、LaTeX 子集、公式编辑语义和字体选择的唯一权威。公式与表格之间的工作边界见[专题索引](math-and-table.md)。

## 公式节点与 LaTeX 语义

公式是可编辑的语义树，不是普通正文中的 LaTeX 源码。Caret 持有稳定、版本化的文档节点与数学树；解析器和导出器是输入输出转换边界，不是第二份文档状态。MathEdit、页面布局、命中测试、光标几何与 Qt 绘制沿 Rust 领域模型、布局快照、FFI 和界面的分层边界协作；不完整或不支持的能力必须拒绝编辑并保留原文档。

首版采用 LaTeX 对应名称：

| Caret 节点 | LaTeX 术语 / 写法 | 位置与语义 |
| --- | --- | --- |
| `InlineMath` | inline math：`$...$`、`\(...\)` | Paragraph 行内原子项；公式内部位置由 Math 编辑适配器维护 |
| `Equation` | `equation*` / `\[...\]` / `$$...$$` | 独占一行的无编号显示公式块；不是可自动折成普通段落的文本行 |
| `Align` | `align*` | 多行无编号显示公式；每行有对齐单元格，`&` 是对齐点，`\\` 是公式行分隔 |

命令面板中的可读别名与公式源码分隔符是两套输入语法，不混用。`\math` 创建行内公式，`\equation` 创建单行显示公式，`\align` 创建多行对齐公式；`$...$`、`\(...\)`、`\[...\]` 是源码导入分隔符，不作为命令候选。命令只有在文档持久化、页面布局、绘制、命中与光标几何都能处理结果时才可执行。

首版只支持无编号显示公式：不实现自动编号、`\label`、`\ref`；编号版 `equation` / `align` 不得静默丢弃编号语义，首版应明确报不支持并保留输入。导入源码只用于解析与错误位置；成功导入后不保留其原始文本。未支持的命令必须报错并保留原内容。正文 Shift+Enter 的 `SoftBreak` 与 `Align` 行分隔是不同结构。旧版双表示字段不得作为未知扩展原样保留；schema 版本或能力检查必须拒绝不符合单一树表示的文档，不做隐式迁移。

MathTree 是 `.cdoc` 中数学内容的唯一权威。文档只保存结构、文本、格式与节点元数据，不保存 LaTeX source、节点 sourceRange 或 TextFlow sourceMap。MathTree 的 schema 版本和文档能力标记必须识别树唯一表示；不匹配的旧版本或遗留源码字段一律拒绝，不能迁移或放入 `extra`。LaTeX 导入由解析器生成 MathTree；导出转换器从树生成规范 TeX，不承诺保留原始空白、注释、命令拼写和定界符。数学树中的文本流导出为数学文本命令（如 `\text{...}`），其中 Run 属性和行内环境再转换为对应的 TeX 文本格式命令；段落中公式之外的普通文本导出为 TeX 正文，不包入 `\text`。这些 TeX 写法属于转换规则，不是 `.cdoc` 的第二表示。

需要源码编辑时，从 MathTree 生成临时 TeX 草稿及节点/文本位置映射，并绑定到对应树版本；候选解析成功后按结构和临时位置对齐，保留可归属的稳定 ID、Run 属性和未知字段，再原子提交新树。解析失败时保留已提交树、草稿和公式焦点。失焦、保存或关闭时必须先提交草稿或由用户明确取消；提交失败则取消相应状态迁移，不静默丢弃草稿。撤销/重做优先作用于草稿编辑历史，因此可用撤销修复不完整 TeX；草稿本地历史耗尽后，只有有效草稿提交后才能继续访问文档历史，提交失败则保留草稿并拒绝跨入文档历史。只有合法树事务进入文档历史。若草稿绑定的树版本已过期，拒绝提交并保留草稿，不覆盖较新树；用户可显式放弃该草稿并离开，已提交树保持不变。以后若支持基于新版本重放草稿，重放冲突也必须保留原草稿供用户处理。TextFlow 模式直接改写目标文本流，不经过 TeX 往返。Tree 原生编辑按 `Mod+Backspace` 降级为源码文本时，空 MathFlow 与无额外元数据的空槽 Placeholder 视为相同空槽，光标映射到 `{}` 内部；未知元数据或真正不透明内容仍拒绝降级。导出遇到无法表达的外观、语义或未知字段时返回明确损失错误，不得静默降级。解析、树对齐或事务失败不得改写已提交公式、有效焦点或草稿。

源码编辑期间的临时映射将 MathTree 路径、显示 TextFlow UTF-8 区间与规范 TeX 草稿关联。转义符号与 TeX 语法会造成长度差异；映射只在该草稿和对应树版本有效，不写入 `.cdoc`。空文本流由零长度 TextFlow 表达，不需要源码映射字段。

### 文档树中的位置

段落中的公式是 `ParagraphContent::Math(InlineMath)`；显示公式 `Equation` 与 `Align` 是 Block 节点。持久化结构字段由[文档树与结构](document-model/document-structure.md)定义；共享 Block/Inline 交互由[块宿主交互](document-model/block-interaction.md)定义；数学载体候选与文本适配见[环境视图](document-model/environment-views.md)。

```text
Document.body: Vec<Block>
├── Paragraph
│   └── inlines: Vec<ParagraphContent>
│       ├── Text(TextFlow)
│       ├── Math(InlineMath { id, math_tree })
│       └── Environment(InlineEnvironment<ParagraphContent>)
├── Equation { id, math_tree }
└── Align { id, rows: Vec<MathAlignRow { cells: Vec<MathTree> }> }
```

Caret 数学树保留 TeX 原子与符号顺序，不解析成代数关系或运算语义。`MathTree { schemaVersion, root, extra }` 的字段与验证由本页定义：`root` 是 `MathFlow`，`MathContent` 按源码顺序交替保存 `Math(MathNode)`、`Text(TextFlow)` 和行内格式环境。分数、根式、脚本等结构节点的每个语义槽位各持有一个有序 `MathFlow`；流内切分不得增加、删除或重排这些固定槽位。行内环境是 `InlineEnvironment<Child>` 包装节点，`ParagraphContent::Environment` 使用 `InlineEnvironment<ParagraphContent>`，`MathContent::Environment` 使用 `InlineEnvironment<MathContent>`；共享包装语义与树操作，子项类型由宿主流约束。MathTree 是唯一持久表示；LaTeX 规范文本由转换器按需生成。文档级能力登记和未知能力行为见[文档存储](document-model/document-storage.md)。早期临时字符串 `TextContent` 格式不属于受支持 schema，也不提供兼容或迁移路径。

### 数学树限制

每个节点只保存自身的原子文本和有序子节点；单棵树受深度 32、节点 65,536 与可变载荷 8 MiB 的持久化上限约束。单次导入或源码编辑草稿另受 1 MiB TeX 输入上限约束，该限制不改变 `.cdoc` schema。行内公式在段落逻辑文本中占一个 U+FFFC（3 字节），作为原子对象参与编辑和布局。若测量显示大量公式文档的内存或编辑成本成为热点，再评估连续 arena 等紧凑表示，不改变 `.cdoc` 编码。

已知 `MathNodeKind` 必须满足当前 schema 定义的操作数数量、槽类型、原子文本和属性值域；缺失/多余操作数、错误槽类型或非法属性属于格式错误，打开时拒绝，不降级成 Opaque 或 SafePreview。无法识别的未来 kind 只有在其原始节点边界可无损保留、宿主位置可可靠定位时才按未知节点进入只读 SafePreview；否则拒绝打开。未知语义不可由当前公式布局或编辑器猜测。数学节点结构验证是存储与模型的同一契约，布局和编辑不得各自推断更宽松的结构。

## 解析、排版与字体

公式后端是工作区内的 `caret-math` crate，选型依据见[研究 010](../../research/010-typst-math-layout.md)：

- **解析与导出**：LaTeX 子集解析器将输入转换为 `MathTree` / `Align`；导出器将文档树转换为规范 TeX。编辑 TextFlow 不经此转换边界；显式源码编辑才使用临时 TeX 草稿和会话映射。树与草稿无法无歧义对齐，或导出无法表达格式与扩展元数据时，返回明确错误并保留有效状态。
- **排版**：`layout/` 移植自 Typst v0.15.1 的数学排版，度量来自 OpenType MATH 表，经 `read-fonts` / `skrifa` 读取。`Align` 的显示布局在调用期间适配成数学后端的 `aligned` 矩阵，由数学表格适配器计算对齐列与行距；Equation 是单行单格的显示公式，Align 是多行多格的显示公式；两者都不转存为 Caret 文档表格。行框和网格纯几何由共享的 `caret-grid-layout` 计算，正文布局与数学宿主向它提供测量结果和行距策略；文档表格以后也复用同一层。嵌套 matrix/cases 保持数学自身排版，不继承宿主正文行高。移植文件的来源与修改记录在文件头，见[源码署名](../../standards/source-attribution.md)。
- **输出**：`MathLayout` 独立产出 Glyph、Rule、Line、Rectangle 和 MathNodeBox 项及其几何；页面布局将这些项适配到 Caret 页面快照，并让绘制、命中和光标共用几何。InlineMath 作为行内对象参与正文段落行框；Equation 作为一行一格、Align 作为多行多格使用正文基础行框和共享网格推进。超出行框的上标、分数等由 Natural、Multiple、AtLeast、Exactly 规则处理。数学 caret 使用宿主的正文基础行高，并锚定 MathNodeBox 或 MathText 当前视觉行的真实基线；上下标等结构保留相对基线偏移，多行文本岛按当前视觉行取基线。Exactly 按宿主行距固定推进，逐行溢出裁切必须与行身份、绘制、命中和 caret 共用同一几何。Equation/Align 的块前后间距沿用普通段落的相邻间距合并规则。空公式宿主（InlineMath 根、Equation 根与 Align cell 根）保留非零 MathNodeBox 作为命中、插入点和 caret 的布局锚点，但不输出内部必填槽使用的描边 Rectangle；分数、脚本、根式等内部必填空槽仍绘制该矩形。这个区别属于稳定的树布局角色，与当前焦点无关。SVG/PNG 不是文档模型，也不是交互路径。
- **缓存**：`MathLayoutEngine` 按公式内容、字号、样式与字体缓存排版结果；页面布局服务负责组合和复用。

数学 `aligned` / `matrix` 的适配器处理 MathFlow 片段、数学字号与 OpenType MATH 度量，并把已测量网格交给 `caret-grid-layout` 定位。正文行度量通过 `caret-layout` 适配到同一共享行框 API；块级 Equation/Align 由 engine 提供正文行距输入，Align 逐行布局而不污染嵌套 matrix/cases 的数学策略。Caret 文档表格处理单元格块序列、可用宽度下的列宽求解、内容重排、分页裁剪与命中；其几何适配器复用 `caret-grid-layout`，但不依赖 `caret-math`。列表布局同样可以把标记列与正文列作为几何输入；编号、缩进、嵌套和跨页连续性由列表领域模型定义。

数学字体的内置默认角色为 STIX Two Math（OFL），三平台不依赖本机字体。文档 `defaultMathFontFamily` 使用统一 `<default>` 字段值；布局与字体选择按[字体选择规范](font-selection.md)解析该标记、显式字体和缺字回退，并为不支持的选择提供诊断。未来扩大支持列表时，应另行处理字体度量、缺字回退、缓存与 `.cdoc` 重现性。


## 公式中的普通文本

数学结构槽位中的 `\text{...}` 是 MathFlow 的文本分支，与数学源码编辑分支平级；InlineMath 根层的 `\text{...}` 按本页宿主规则提升为 Paragraph TextFlow。Paragraph 行内流使用 `Vec<ParagraphContent>`，包含 `Text(TextFlow)`、`Math(InlineMath)` 和 `Environment(InlineEnvironment<ParagraphContent>)`；其他语义分支由各自的文档模型定义。MathFlow 与 Paragraph 共用 TextFlow 载体，不建立第二套文本存储或编辑器。TextFlow 只保存文本 Run；Math 和 InlineEnvironment 是同一父序列的其他分支。

`TextFlow` 复用正文 Run、Run 属性、未知字段和字素边界；Paragraph 与 MathFlow 使用同一 TextFlow 文本、Run 属性、InlineEnvironment 和字素编辑适配器。Paragraph 与 MathFlow 宿主适配器只解析宿主路径、定位节点并提交各自的历史事务；MathEdit 不另实现一套文本格式语义。源码编辑映射属于临时视图，不是 TextFlow 或 MathTree 的持久字段。

数学字面符号按扩展字素簇划分编辑位置，连续字母和数字之间允许停靠与逐个删除；命令生成的符号和复合运算节点保持原子。解析输入与逐键输入使用相同粒度，不能因解析器合并字面内容而改变光标和删除行为。

IME 只在活动分支为 TextFlow 时启用。数学 TextFlow 请求 Qt `ImhPreferLatin`，用于提示输入法优先提供拉丁字符；这是平台提示，不能锁定或切换宿主操作系统的输入法，也不应阻止用户提交 Unicode 字符。MathTree 结构分支、数学源码和数学命令查询仍拒绝预编辑、替换或组字预览；不能只打开输入法提示就启用结构分支 IME，后续若支持，须先定义结构插入点的 surrounding text、UTF-16 替换范围、预编辑投影与原子提交。Qt 的 `ImEnabled` 与 `ImHints` 由 Rust 活动分支投影，TextFlow 的替换事务遵守正文输入法语义并由宿主事务提交；公式文本继续使用正文相同的 shaping 与缺字 fallback。

公式源码分支拒绝正文格式命令，避免误改宿主段落的 pending style。TextFlow 分支的 Run 属性格式（例如 underline）由 Run 属性适配器在目标 TextFlow 内更新：选区操作只拆分受影响的 Run，保留其他 Run 元数据；未知字段无法安全复制时整笔事务拒绝。折叠光标使用统一 `PendingInlineStyle`，新输入写入 Run 属性，焦点或选区变化时清除待输入格式。Bold、Italic、Font 使用 `InlineEnvironment` 格式适配器；相交范围通过树切分表达，不生成交叉包装。数学源码分支明确拒绝不支持的正文格式命令，不得静默清除 MathEdit 后落到宿主旧选区。固定数学槽位内的 TextFlow 从 MathTree 内部环境解析继承样式；Paragraph 外层环境不改写 MathTree；公式内部 TextFlow 的格式由 MathTree 内部环境决定。

### 宿主边界与拆分

正文中的 `\math` 命令创建行内公式。Paragraph 是 InlineMath 根层 `TextFlow` 的唯一编辑宿主：InlineMath 根 `MathFlow` 不持有可提升的 `TextFlow`。根层透明 `Row` 按其子流展开后再判断提升，不能用它包装正文文本来绕过宿主规则。归一化按来源顺序形成相邻的 `ParagraphContent::Math` 与 `ParagraphContent::Text`；相邻数学内容合并为一个 InlineMath 片段。拆分只递归穿过 MathFlow 根流、透明 `Row` 与 InlineEnvironment，不进入 MathNode 的分数、根式、上下标等固定操作数槽位。

拆分递归穿过 Bold、Italic、Font 等 `InlineEnvironment`，保留单个环境包装、ID、顺序和语义。例如 `\math(\bold{a\text{x}b})` 归一化为一个 Bold 包装，children 按序为 Math、Text、Math；不得拆成三个相邻 Bold，也不得把 Text 留在 InlineMath 中。`Run.props`（例如 underline）随 TextFlow 的 Run 保留，不转换成 InlineEnvironment。

数学树不持久化源码范围或 `sourceMap`，数学节点中的旧版映射字段由存储边界拒绝；正文 TextFlow 不定义数学源码映射，其同名未知字段仍按普通扩展元数据处理。源码编辑适配器先通过与树版本绑定的临时映射把光标和选区映射到各输出片段，再创建宿主位置；移除的命令包装映射到最近的可表示边界。Text 后紧邻 Math 的源码起点归属前一正文 TextFlow 末端。若 anchor 与 focus 分属不同输出片段且不能由宿主位置模型无损表达，整笔事务拒绝。根层 `\text{}` 保留为空正文 TextFlow；文本岛中显式的空数学区间保留为可编辑空 Math 片段。无法归属到输出节点的临时源范围或未知字段必须导致事务拒绝，不得静默丢弃。

归一化、源码解析/序列化、焦点与选区映射、节点身份和历史补丁属于一个原子文档事务。分数、根式、上下标等内部槽位的 `\text{...}` 始终留在各自 MathFlow 中。在这类 TextFlow 内执行 `\math` 时，共享流操作把选中或插入范围切分为前后 TextFlow 与新的 MathNode；后续输入进入数学分支，不提升到 Paragraph。需要源码编辑时，才通过临时 TeX 草稿和位置映射转换整棵树。Equation/Align 没有 Paragraph 宿主，其根流保留数学混合内容。

### split/merge 设计

split/merge 操作接收强类型有序流和宿主路径，由宿主适配器访问节点与位置；ParagraphContent 与 MathContent 共用操作策略但不抹平成无类型数据。Bold、Italic、Font 声明包装语义，不各自复制拼接算法。抽象必须维持宿主位置映射、元数据校验和事务边界。

根流提升时，`InlineEnvironment<MathContent>` 转为 `InlineEnvironment<ParagraphContent>`，输入包装仍对应一个输出包装，不因子项类型转换复制节点。只有编辑范围使同一包装包含多个不连续输出范围时，才拆成多个包装；原 ID 留在首个存活的同 kind/同语义包装，其余新节点使用新 ID。InlineMath 宿主 ID 留给首个仍存在的 Math 输出项；没有 Math 输出时移除该 ID。MathNode 内部 ID 原样保留，无 ID 节点不生成身份。Run 与环境未知字段按实际归属保留；不能证明安全复制或唯一归属时整笔编辑原子拒绝。合并仅在属性、未知字段和身份可无损处理时进行。

文本岛复用正文 shaping 的字体回退、语言、Run 属性与 InlineEnvironment，但不获得段落块布局、段落前后距或分页语义。Bold、Italic、Font 的 `enabled` / `fontFamily` 语义由 `caret-doc::InlineTextStyle` 统一解析，正文 shaping 和数学布局适配器都消费同一份增量；数学 backend 只负责把它投影到 TeX 命令，布局侧直接使用结构化样式，不再往返解析 `bf/md/it/up` 字符串。数学文本不维护专属字体规则：`\textsf` / `\texttt` 映射为 TextFlow 的通用 Font 环境；`sans-serif` / `monospace` 遵循[字体选择规范](font-selection.md)中的通用族语义，和 Math/Code 字体角色相互独立。公式文本与正文采用相同的 shaping、字体度量和缺字回退行为；布局快照 `textDiagnostics` 使用同一诊断结构报告正文与公式的缺字/缺失字体族，并附带段落或公式宿主 ID。

| 内容 | 数学源码表示 | 编辑/导出规则 |
| --- | --- | --- |
| MathFlow 中的 TextFlow 与行内格式 | `\text{...}` 及 TeX 文本格式命令 | 公式结构操作数内的 TextFlow 是可用 IME 的 MathText；InlineMath 根层选择 `\text` 时提升为段落正文并拆分两侧公式。删除该文本岛后仅在安全条件满足时合并本次相邻公式，并把焦点放回合并后的 MathTree 接缝。导出器从 TextFlow 与格式语义生成规范 TeX；命令名称不进入 `.cdoc`，等价输入可归一为同一树 |
| Run 的 language、underline 与未知扩展字段 | 普通 LaTeX 子集通常不能完整表达 | Run 属性和未知扩展字段沿稳定节点身份及结构路径保留；无法唯一对齐时拒绝提交。标准 LaTeX 导出遇到影响外观或语义的元数据时返回明确损失错误 |
| 数学文本岛中的 Font 环境 | TeX 可用 `\textsf` / `\texttt` 表达部分字体族 | 解析为与正文相同的 TextFlow Font 环境；`\mathsf` / `\mathtt` 保持数学字母表语义。TeX 表达不了的文档属性按通用导出损失规则处理；字体候选与 fallback 见[字体选择规范](font-selection.md) |
| 编辑器命令 `\math` | 非 LaTeX 源码语法 | 正文和公式 TextFlow 编辑模式都可用的结构命令；`.cdoc` 只保存转换后的语义结构，不保存命令面板别名或 TeX 源码 |

不得用重复文本的模糊搜索匹配节点身份。源码编辑草稿与树元数据的对齐必须使用解析结构和绑定树版本的会话位置映射；无法唯一映射时拒绝提交，保留原树和焦点。数学节点的布局使用统一 MathBox 几何契约，并保持树位置、光标、命中与选区映射一致。

共享 TextFlow 不构成性能收益证明。性能结论必须基于代表性输入与可复现基线：固定机器、工具链和构建配置，测量长纯文本、密集短 Run、数千短行内公式、中日韩与组合字符文本岛、深层嵌套及接近节点/源码限制的大公式；记录解析/序列化与打开/保存耗时、输入/拆分/撤销延迟分布、分配次数、峰值内存、枚举尺寸和 `.cdoc` 字节数，再判断是否需要间接存储或局部撤销快照。

## 导航与编辑

本节定义跨实现的数学编辑用户行为。

活动 Equation/Align 的块背景与 TextFlow 焦点高亮属于画布交互投影，其几何和绘制复用规则统一见[画布与交互背景](../interface/canvas-highlighting.md#活动公式焦点)。

- `\math`、`\equation`、`\align` 分别插入行内公式、单行显示公式、多行对齐公式，并进入公式编辑。命令仅在持久化、页面布局、绘制与光标路径均可处理结果时可执行。
- 正文插入点输入 `$` 创建并聚焦一个空行内公式；它是编辑器的输入字符行为，不绑定物理 `Shift+4`。命令查询中的 `$` 保持查询文本，公式编辑中的 `$` 作为公式字符输入；`$...$` 仍是源码导入语法，不代表成对定界符的正文输入流程。`、`、`¥` 和 `￥` 均为普通文本。
- 公式编辑状态记录目标公式、当前编辑分支和该分支的位置。源码模式使用 UTF-8 字节位置；TextFlow 模式使用正文文本位置语义。切换模式时，适配器用公式树位置映射恢复焦点；失败时保留原文档与有效位置。
- 数学文本选区使用 UTF-8 字节偏移设置 anchor/focus；两端必须落在 grapheme 边界。输入和退格/删除替换选区，左右键将选区折叠到对应边界。仅在源码模式切换或提交时，若临时映射不连续或无法无损覆盖，操作原子拒绝。跨 TextFlow/InlineEnvironment 的选区由宿主路径映射到同一逻辑文本范围；公式原子与相邻文本之间必须保留可用 caret 边界。布局命中和选区绘制使用相同的 MathBox 几何。
- 左右键在花括号组、token 边界间移动；控制词整体跳过与删除。到达边界再按一次离开公式；方向键、鼠标、选区命令、Esc 与上下文允许的 Enter 可退出公式。Esc 离开块级公式时落在当前公式的外后停靠；连续按 Esc 保持该停靠，不越过相邻宿主，需用方向键继续移动。Enter 的块级退出按其明确的插入语义处理。公式对象删除只删除焦点对象并按文档结构恢复焦点，不复用 Esc 的边界转换。
- Up/Down 在 MathTree 中根据同一公式布局快照生成垂直候选，使用结构节点、分数槽位及脚本的实际基线和光标高度；候选只进入可见的正尺寸结构槽，必需 Placeholder 可在其前定位、不会生成其后的插入点。左右键跨入或离开 MathFlow 时也不会停在 Placeholder 后。Session 对鼠标命中、导航和输入统一约束紧邻 Placeholder 的非法边界，同时保留 Placeholder 后其他已有内容的合法位置。Align 同一行的不同 cell 不视为上下目标，跨行按宿主行和页面几何选择。Session 通过统一 `EditorKey` 入口校验目标公式与布局代际后提交。MathFlow 内 TextFlow 继续使用 Parley 正文文本移动语义并适配宿主路径。MathTree 内部位置由数学适配器维护，选区端点保留完整树路径；跨出数学文本岛或公式宿主时映射为共享文档范围。MathTree/Source、MathText 与文档级范围使用同一选区协议衔接，不能因局部坐标不同而静默忽略扩选或丢失所选公式对象。没有内部垂直候选时，Up/Down 与左右键的边界规则一致：先退出公式编辑并把焦点映射到共享文档位置（InlineMath 为 U+FFFC 前/后侧，块级公式为块外侧停靠），InlineMath 由 app 在同一次按键内继续文档纵向导航；Shift+Up/Down 对 InlineMath 先退出再按文档选区协议扩展，Block 的垂直扩选仍由布局候选决定，无候选时保持原行为。
- Equation/Align 的块边界进入/退出使用[结构导航](structure-navigation.md)规定的共享 Block 仲裁；数学适配器只提供 MathTree/MathText 内部移动候选和同代公式几何，不再维护公式专用块跳转状态。
- InlineMath 是 Paragraph 内的 U+FFFC 原子 Inline。外部两侧及公式内部位置按[文档位置](document-model/document-position.md)映射；它通过共享 Inline 范围底座参加选区与删除，不使用复杂 Block 的边界停靠。Inline 高亮与空槽位规则以[画布高亮](../interface/canvas-highlighting.md)为准。
- `_` 与 `^` 插入带空组的下标与上标，`{` 自动补 `}`，`}` 跳出组。
- 公式编辑中 `\` 打开数学命令，不打开正文命令；`Align` 中 Enter 插入 `\\` 结束一行，`&` 为对齐点。
- 退格行为按焦点对象分别仲裁：MathTree 根为空时删除整个 InlineMath、Equation 或整个空 Align 对象；源码分支中的空公式也删除对应公式对象。Align 的 Backspace 按逻辑行优先处理：当前行的所有 cell 均为空时删除整行，焦点落在前一行末端；若删除首行则落到新首行起点；唯一的空 Align 仍按空公式宿主规则删除整个对象。删除行前检查被移除的行、cell、MathTree、MathFlow、TextFlow 和 Run 元数据；无法安全丢弃时原子拒绝。非空 Align 行在首 cell 逻辑起点处 Backspace 不产生任何状态变化，不合并内容、不跨行移动；行内其他空 cell 的退格仍按 cell 导航规则处理。逻辑行退格策略由块编辑共享，Align Tree、Text 和有效源码分支映射各自焦点与行结构；源码行含注释或元数据无法安全映射时保留原状态。该规则面向拥有内部逻辑行/cell 模型的块，不由视觉多行或 `MultiLine` 索引分类推导：Section 是标题与子块容器，其中 Paragraph 保留段落退格规则。空构造参数（例如 `\\sqrt{}`）先按公式内部退格规则移除构造，光标位于空 MathText 文本岛时保留该文本岛，不删除宿主公式。结构选区的块删除、普通 Paragraph 的空段删除和 MathTree 根删除不共享“空对象”判定。只移动插入点的离开操作不产生撤销步；修改文档树的操作才记账。删除公式对象的撤销恢复公式编辑目标、焦点和视觉 affinity，重做恢复删除后的宿主焦点。
- Tab 保留公式编辑目标，不向宿主段落插入空格；槽位导航需由明确的键盘契约定义。
