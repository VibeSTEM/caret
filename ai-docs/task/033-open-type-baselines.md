# 033 — OpenType BASE 脚本基线与行框溢出

- 状态：in-progress
- 依赖：024、026、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-30

## 目标与背景

修正跨脚本、字号和字体回退时的基线对齐，并使行框按真实排版内容而非偶然偏大的字体全局度量计算。触发场景包括在拉丁段落输入中文全角标点导致行或后续段落下移、Arabic 段落沿用 Latin 锚点，以及较大字体/组合附标/回退字形超出行框却未标记。

本任务采用两项公开依据：

- [OpenType BASE 表](https://learn.microsoft.com/en-us/typography/opentype/spec/base)：字体可提供 dominant run 相对的脚本基线、脚本/语言/特性的 min/max extent；BASE 值是建议值，客户端仍负责选择 dominant run 和缺失数据时的策略。
- [Microsoft Word 段落间距选项](https://support.microsoft.com/en-us/word/adjust-indents-and-spacing)与[默认行距](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)：仅据此记录官方定义的 Before/After、Multiple、At least、Exactly 语义及当前默认行距 1.15。官方页面不规定通用的段前/段后 pt 默认值，也不公开行距网格字体或 BASE 选择算法。用户报告和截图作为 Caret 需求证据单独标记，不写成 Microsoft 官方规则。BASE 用于脚本基线对齐，不把同字号 fallback 字体 extent 纳入行距。

目标是保持 Word 风格行距选项的可观察含义，并使用 OpenType BASE 对齐多脚本；不是直接对齐 OOXML 存储格式，也不保证任意系统字体与 Word 像素级相同。

## 必读

- [页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [分层](../standards/layering.md)、[验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)
- [OpenType BASE 表](https://learn.microsoft.com/en-us/typography/opentype/spec/base)，Microsoft Learn，查阅日期：2026-10-30
- [Word 段落间距选项](https://support.microsoft.com/en-us/word/adjust-indents-and-spacing)与[默认行距](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)，Microsoft Support，查阅日期：2026-10-30
- `Cargo.lock` 中的 `read-fonts 0.41.0`；该 crate 已是间接依赖，实施时核对其 BASE API 后再登记为直接依赖。不手写 OpenType 表解析器。

## 范围与非目标

本任务处理：

- 按段落完整逻辑文本选择稳定的 dominant script/run；软换行不重选锚点。
- 从实际 shaping run 的字体 face、face index、字号、语言与 variation 实例读取水平 BASE 基线信息，按 dominant baseline tag 计算其他脚本 run 的相对基线偏移。
- 使用 BASE 为不同脚本 run 对齐 baseline；同字号 fallback 的字体全局 ascent/descent/leading 与 BASE script extent 只用于基线映射和越界诊断，不扩大行框。行距网格基于文档/段落基础样式字体及字号，按稳定的 Latin 基准解析，不随正文 Script 或 Paragraph.language 变化；只有明确更大字号或行内对象按 Word 公开的行距模式参与自然行高/下限。
- Microsoft 官方文档列出的 Multiple、At least、Exactly 及段前/段后语义作为参考；Caret 的 Natural/Single 和 fallback 字体策略属于本产品规则。保持用户实测的同字号跨语言行距一致：fallback 不构成更大字号；At least 只因真实字号/图形需求或显式下限而扩大；Exactly 保持固定 pt 推进。
- 处理缺少 BASE 表、对应 Script/baseline tag、语言数据或可解析 extent，以及损坏字体表的确定回退；错误数据不能按零偏移静默处理。
- 记录行框溢出并确保基线偏移、绘制、选区/光标几何和分页消费同一份坐标结果。
- 为 BASE 与 glyph 度量数据建立有界缓存，避免按字形反复解析字体表；字体刷新后失效。

不包含字体/语言选择 UI、DOCX 导入导出、纵排、用户自定义 BASE 或重做 Parley shaping。不把 Word 未公开的内部算法写成兼容承诺。Caret 的默认段前 0 pt、段后 8 pt 属于产品预设，不声称是 Microsoft 对所有模板规定的默认值。

## 前置条件与待决策

024 字体 shaping、026 行距/分页、027 glyph 绘制均已实现。开始时需核对 Parley、Fontations `read-fonts` 和 Swash 实际锁定 API，以及当前 Parley run 是否能提供实际字体 face、glyph ID、字号、variation 坐标和稳定 font resource ID。

用户报告在 Word 中不同语言的行间距一致；Caret 的截图显示中文选区框高于英文，说明当前行框几何也有差异。两者不能混为一个推断：先以用户观察作为目标行为，再以可控字体和行框/选区坐标测试 Caret。Word 版本、字体与具体行距设置尚未记录，后续可补录受控对照；不推断 Word 私有算法。

### 固定设计规则

1. **dominant script/run**：先在整段 UTF-8 逻辑文本（跨软换行）找到第一个 Unicode 强 Script grapheme；`Common`、`Inherited`、`Unknown` 不选作锚点。首个强脚本为段落锚点，实际塑形中该脚本首个 run 的 resolved font face、字号、语言和 variation 实例组成 dominant run。整段没有强脚本时使用稳定的 Latin/系统字体度量回退。`Paragraph.language` → 文档 `defaultLanguage` 决定 shaping 与 BASE 语言系统上下文，但不覆盖文本里实际出现的第一个强脚本。
2. **Common/Inherited**：Script 归属跟随塑形 span；前缀 Common 字符使用后续首个强脚本，其他 Common 字符沿用前一个强脚本，组合附标留在其 grapheme 内。它们不改变 dominant run。
3. **BASE 对齐**：只处理水平 `HorizAxis`。dominant script 的 `defaultBaselineIndex` 指定 alignment tag；其他脚本在自己的 `BaseValues` 中查同一个 tag，并相对各自默认 baseline 计算偏移。按对应字体 units-per-em 与 run 字号缩放，并转换到 y 向下的 Caret 坐标。缺少任一所需 tag/坐标时，该 run 使用 Parley 字体度量回退，不能改用全局 Latin font 的数据。
4. **BASE 格式与 variation**：对能在文档 pt 坐标中准确解释的设计单位坐标应用 BASE 值。依赖 contour point hinting、设备像素或当前实现不能解析的 variation delta 的坐标回退到字体度量；不得缓存或使用错误 variation 实例的数据。后续支持相关格式时需新增对应 fixture 与版本记录。
5. **行高与行距**：Caret 的 Natural/Single 使用基础样式字体的共享行距网格；同字号跨脚本 run 经 BASE 对齐但不扩大网格。明确更大字号与行内对象按行距模式增加所需空间；Multiple 以基础样式单倍网格乘倍率；AtLeast 应用显式下限并容纳更大字号/对象；Exactly 固定 pt 推进。真实 glyph ink 越出行框时记录 overflow，但同字号 CJK ink 下伸不能移动下一行、段落或增大选区行高。段前/段后为独立固定 pt 值，本任务的产品默认是段前 0 pt、段后 8 pt，且均可自定义。
6. **字体 extent 与 ink**：BASE script/language MinMax 和实际 glyph ink bounds 可用于验证哪些墨迹越出共享行距网格，不将它们作为同字号 fallback 的自动行高来源。emoji 彩色 bitmap 或不可读取轮廓时保留明确的度量来源/overflow 诊断。不得为每次按键遍历整字体 glyph 集合。
7. **缓存**：键至少包含 `FontData` blob identity、collection face index、Script、alignment tag、语言上下文与 variation coordinates；按 font generation 清除，容量有界。键与缓存值由排版层拥有，不进入文档或 QML。

## 实施步骤

1. 先完成受控 Word 对照并记录 Word 版本、字体文件/版本、段落行距设置、导出 PDF/几何测量方式及结果；更新本任务与[页面排版架构](../architecture/editor/page-layout.md)中需要修订的设计。
2. 核对锁定 `read-fonts` API 对 BASE 字段和坐标格式的支持；选择分发许可明确的固定测试字体，覆盖含 BASE 与不含 BASE 的字体。确认 collection face 与 TTC/OTC face index 一致。
3. 将 Script/dominant run 选择、BASE 字段读取、字体度量/ink 范围和行框策略拆成边界清楚的模块；不让 `caret-engine`、FFI 或 QML 各持一份基线策略。
4. 增加固定字体与合成数据测试：Latin/Hani/Arab 混排、Common 标点、字号差异、缺失/损坏表、脚本/tag 缺项、语言 extent、variation 回退、软换行、组合附标、大字号溢出。
5. 验证同一基础样式下独立 Latin 段、Hani 段和混排段的行框、选区高度、后续行 baseline、段落起点一致；改变 Paragraph.language 不改变网格。BASE 脚本偏移只影响 glyph/caret/selection 的共同坐标映射，不改行推进。更大字号、对象与 Natural/Multiple/AtLeast/Exactly 按定义处理。
6. 记录 BASE 解析 cache miss/hit 的可复现耗时或分配量、大文本代表性输入和容量；证明按 run/font-face 缓存而不是每 glyph/按键解析。无测量结果不得宣称性能提升。
7. 按验证规范运行定向 Rust、native/UI 测试及 lint；更新文档结果、task-index，并由未参与实现的 subagent 做 Sol medium 独立 final review，修复后复验。

## 预计改动

预计影响 `crates/caret-layout/` 的 BASE 解析、glyph 范围和行度量，`crates/caret-engine/` 的布局缓存与页面 glyph 集成测试、固定字体 fixture、`Cargo.toml` workspace 依赖，以及 `ai-docs/architecture/editor/page-layout.md` 和本文。新增依赖仅在当前锁定的 `read-fonts` 版本 API 可用且确有必要时添加；不新增自制解析器或为缩短文件创建 crate。

## 验收标准

- [ ] dominant run、强脚本与 Common/Inherited 处理有确定规则，段落软换行不重选锚点。
- [ ] 有 BASE 对应项的 run 按该字体实际 face/字号和 dominant alignment tag 获得稳定的脚本基线偏移；缺项/坏表/未支持坐标格式走确定字体度量回退。
- [ ] 中英混排标点和输入/删除不移动无关 Latin glyph；Arabic 文本不被全局 Latin 锚点覆盖。
- [ ] 相同基础样式与字号下，Latin、Hani 与混排的行框/选区高度、行推进和后续段落起点一致；更改正文语言标签不改变网格。同字号 fallback 的字体全局度量或 BASE extent 不扩大行距。更大字号/对象按行距规则影响 Natural/AtLeast；Exact 固定推进并正确报告 ink overflow。
- [ ] Multiple/Exactly 保持定义的推进；同字号多语言不改变倍率基准，文字 ink 下伸不移动后续行；溢出能被诊断且分页行为稳定。
- [ ] 默认段前/段后样式值不被行高代码改写；同字号跨脚本视觉 y 差异仅体现 BASE glyph 偏移，不改变相同段落的行距和段落起点。
- [ ] 行距公开语义与 Microsoft Word 文档一致；受控 Word 对照记录可复现，偏差有说明，不声称复刻未公开内部算法。
- [ ] 固定测试字体随仓库许可说明可分发；坐标测试不依赖开发机的系统字体版本。
- [ ] BASE/extent 缓存有界并按字体代际与 variation 坐标失效；实际耗时/分配测量覆盖长文档布局，没有增加逐字形表解析。
- [ ] 独立 final review 完成，问题已修复并复验；旧全局 Latin anchor 的实现、测试和过时文档说法已清理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-30 | 用户报告 Microsoft Word 中不同语言行间距一致；Caret 截图对照 | 同一基础样式下 Latin/Hani/mixed 的行框、选区高度及后续段起点相同 | Word 版本、字体与设置待补；Caret 现状确认同字号 run fallback 最大度量会扩大行框 |
| 2026-10-30 | `cargo test --offline --locked -p caret-layout -p caret-engine -p caret-doc` | 同字号跨脚本行框与选区一致、空段落/非空段落行网格一致、段距及分页行为测试通过 | 142 项 `caret-engine`、84 项 `caret-layout`、29 项 `caret-doc` 测试通过；包含页面选区框共享行框、段后 8 pt 默认值 |
| 2026-10-30 | `cargo clippy --offline --locked -p caret-layout -p caret-engine -p caret-doc --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | Rust lint、格式与差异空白检查通过 | 通过；覆盖 Criterion target 编译 |
| — | native/Qt glyph scene 测试 | Qt 绘制、选区与光标使用 Rust 输出的同一偏移和裁切范围 | 未执行；实现未开始 |
| — | 固定混合语言长段落 Criterion benchmark | 记录 BASE 缓存冷/热态耗时与分配量，无逐字形 BASE 表解析 | 未执行；实现未开始 |

## 风险与回退

Word 的行距网格字体选择和内部 dominant-run 算法没有公开保证；以公开行距语义和用户观察到的同字号多语言行距一致作为 Caret 的行为目标。Caret 将基础样式字体行距网格与脚本 BASE 基线锚点分离。BASE 表不是所有字体都提供，格式 2/3、variable font 与颜色 emoji 需要依据 locked API 实测；同字号 fallback 缺少 BASE 时回退到确定的基线对齐，但不能拿字体全局度量扩大行框。overflow 只表示 ink 与行距网格边界不同，不代表必须裁掉 Natural/Multiple 下可正常显示的墨迹。

## 决策与工作记录

- 2026-10-26：从 027 拆出脚本基线与大字号/字体回退行框问题；依赖 024、026、027 已满足。
- 2026-10-29：记录段落语言与强脚本前置规则；后续反馈显示使用所有 fallback 字体最大 ascent/descent 造成中文段落间距明显变化，保留至本任务验证和修复。
- 2026-10-30：将行距网格与 Script dominant run 分离。用户报告 Word 不同语言行间距一致，Caret 截图显示中英文选区行框高度不同。行距网格改由基础样式字体与字号确定，首个强 Script 的实际 run 只作为 BASE 基线锚点；同字号 fallback extent 不抬高行框。公开资料不足以断言 Word 内部使用哪一种网格字体，文档只约定 Caret 的目标行为。

## 完成摘要

未完成。设计按 Word 公开语义和用户实测完成修订；待实现、回归验证、性能测量及独立评审。
