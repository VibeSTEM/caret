# 055 — OpenType BASE 脚本基线与字形溢出诊断

- 状态：ready
- 依赖：024、026、027、033
- 优先级：P2
- 创建 / 更新：2026-10-30 / 2026-10-30

## 目标与背景

在 033 的共享行距网格基础上，为不同文字脚本实现由 OpenType BASE 数据驱动的字形基线对齐，并诊断字形墨迹相对行框的溢出。该任务不能改变同字号跨语言共享行框、段落间距或分页推进规则。

Microsoft 的 OpenType BASE 文档定义字体可提供的脚本基线数据，并说明如何描述不同 baseline 之间的关系；实际采用何种 dominant run、缺少数据时如何回退，属于 Caret 的排版策略，不是 Microsoft Word 已公开的内部算法。Word 行距文档只用于其明确记载的行距类型语义，不用于推断 BASE 选择。

## 必读

- [页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [033 共享行距网格](033-open-type-baselines.md)、[分层规范](../standards/layering.md)
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)
- [OpenType BASE 表](https://learn.microsoft.com/en-us/typography/opentype/spec/base)，Microsoft Learn，查阅日期：2026-10-30

## 范围与非目标

- 按一个段落完整逻辑文本（跨软换行）确定 dominant script/run；以首个强 Unicode Script grapheme 为锚点，`Common`、`Inherited`、`Unknown` 不单独决定锚点。实际塑形 run 的 resolved font face、face index、字号、语言和 variation 实例用于查询对应字体数据。
- 使用 Parley/Fontations 已锁定的字体解析库读取水平 `HorizAxis` BASE 信息；评估需直接依赖 `read-fonts` 时只增加已有库依赖，不手写 OpenType 表解析器。
- 以 dominant script 默认 baseline tag 为共同对齐 tag，在各实际 run 字体 face 中查找数据并换算到 Caret 坐标。数据缺失、损坏或无法准确解释时采用确定回退，不伪造零偏移、不借用无关字体数据。
- 行框仍由 033 的共享网格与段落行距规则决定；BASE 只影响脚本字形在行内的对齐。实际 glyph ink bounds 用于 overflow 诊断，不以同字号 fallback 度量扩大行距。
- 让字形、命中测试、光标、选区及分页消费一致的基线坐标映射；按字体 face/脚本/tag/variation 设计有界缓存并在字体代际变化时失效。

不实现纵排、字体选择 UI、DOCX BASE 映射或未公开的 Word 算法复刻。不为支持某种字体格式而在布局热路径中逐 glyph 解析 BASE。

## 实施步骤

1. 核对锁定 `read-fonts` 对 BASE 结构及坐标格式的支持，并确认 Parley 可提供实际字体 face、face index、字号和 variation 标识。
2. 定义 dominant run、Common/Inherited 归属及缺失/损坏数据回退规则；用确定输入验证算法。
3. 在 `caret-layout` 增加 BASE 读取、run 基线偏移和 ink overflow 诊断，复用同一结果计算 glyph、caret、selection 与 hit-test 几何。
4. 增加有许可说明的固定字体 fixtures，覆盖带 BASE 与不带 BASE 的字体、多脚本混排、标点、字号差异、软换行、缺失/损坏表及回退。
5. 验证不同语言 glyph baseline 对齐时，行框高度、行距、段后距离、分页起点保持 033 规定的共享网格；更大字号/对象仍由既有行距策略处理。
6. 记录代表性长文档下 BASE 缓存冷/热态耗时或分配量，确认没有逐字形解析退化；执行必要的 Rust/Qt 直接消费者测试并更新架构文档。
7. 由未参与实现的 subagent 对完整 diff 做独立 final review，修复后复验。

## 验收标准

- [ ] dominant run 和 Common/Inherited 处理确定且跨软换行稳定；Arabic 等以自身首个强脚本作为锚点。
- [ ] 支持的 BASE 值按实际字体 face、字号和 units-per-em 换算；缺失 tag、缺表、损坏表及未支持坐标格式走确定回退。
- [ ] BASE 对齐不会改变同字号 Latin、Hani 与混排文本的行框高度、选区高度、行推进、段间距或段落起点。
- [ ] glyph、命中、caret 和 selection 使用同一基线结果；单独插入全角标点不会移动无关文字。
- [ ] ink overflow 有可验证诊断；Natural/Multiple 不因同字号 fallback ink 改写共享网格，Exactly 保持固定推进与既定裁剪规则。
- [ ] 字体 fixture 可分发且不依赖开发机系统字体；覆盖 BASE 与 fallback 行为。
- [ ] BASE 缓存有界、按字体代际及 variation 标识失效；长文档测量未显示逐字形表解析或显著回归。
- [ ] 相关行为测试、格式和 lint 通过；架构/任务文档准确描述已实现边界；独立 final review 通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 字体解析 API 检查 | 锁定依赖能读取所需 BASE 数据及实际 face | 待实现前核验 |
| — | 固定字体 Rust 布局行为测试 | BASE 有效/缺失/损坏时行为确定，行距网格不变 | 待实现 |
| — | 混合语言长文档 Criterion 对比 | 记录缓存与布局耗时/分配，确认无逐 glyph 解析 | 待实现 |
| — | Qt/native 几何消费者测试 | glyph、caret、selection 与 hit-test 坐标一致 | 待实现 |

## 风险与回退

字体库可能无法暴露 Parley 实际使用的字体实例或全部 BASE 格式。遇到不可准确解释的数据时回退到 033 的共享行距网格和既有 Parley 基线映射；不得让字体元数据错误扩大段距或行距。固定字体 fixture 需确认许可允许仓库分发。

## 决策与工作记录

- 2026-10-30：从 033 拆分未实现的 OpenType BASE 与 ink overflow 工作。033 只保留并验收已实现的共享行距网格；本任务负责真正的 BASE 数据解析与脚本基线偏移。
- 2026-10-30：公开依据限于 Microsoft OpenType BASE 规范；不把 Caret 的 dominant run 与回退规则描述成 Word 的内部实现。

## 完成摘要

未完成。当前代码已实现共享行距网格，但尚无 OpenType BASE 表读取、脚本间 BASE 偏移或 ink overflow 诊断。
