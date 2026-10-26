# 033 — OpenType BASE 脚本基线与行框溢出

- 状态：ready
- 依赖：024、026、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-26

## 目标与背景

修正跨脚本、字号和字体回退时的字形基线对齐，并保证真实字形范围不会穿入相邻行而布局器仍报告无溢出。触发场景包括在拉丁段落输入中文全角标点导致既有文本或后续行垂直移动、Arabic 段落错误沿用全局拉丁锚点，以及较大字体/回退字体的 ascent 或 descent 超出当前行框。

[页面与段落排版](../architecture/editor/page-layout.md)规定了 Caret 的目标语义。参考 Microsoft 发布的[公开 OpenType BASE 规范](https://learn.microsoft.com/en-us/typography/opentype/spec/base)：以一个 script/size dominant run 为参照，对齐其他脚本的基线；BASE 是推荐度量，客户端仍需定义 dominant run 选择和缺失数据的回退。当前全局拉丁度量锚点实现是临时方案，不作为本任务的设计依据。

本任务属于 027 之后的排版正确性工作，不阻塞 027 的画布、glyph 显示和窗口验收。

## 必读

- [页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [分层](../standards/layering.md)、[验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)
- [OpenType BASE 表](https://learn.microsoft.com/en-us/typography/opentype/spec/base)，Microsoft Learn，查阅日期：2026-10-26
- 当前锁定的 Parley API 与 Fontations `read-fonts` API；实现前以 `Cargo.lock` 版本核对。Parley run 可提供实际字体资源与字号，`read-fonts` 提供 BASE 表读取，禁止手写 OpenType 表解析。

## 范围与非目标

本任务处理：

- 从实际塑形 run 的字体面读取水平 BASE 数据，按 Caret 规则选择 dominant run，并计算不同 Script run 相对主基线的字形偏移。
- 明确 Paragraph/文档有效语言、强脚本、Common/Inherited 字符、字号及多个候选 run 时的 dominant run 选择与稳定性规则。
- 缺少 BASE 表、缺少对应 Script/baseline tag 或解析失败时，应用确定且可测试的字体度量回退；不能把错误数据当作零偏移而静默误排。
- 让自然行框、固定行距、裁剪与溢出检查使用最终字形基线和真实字体/对象度量。保持行距和分页推进规则，不因标点或回退字体偶发变化而移动同段落中既有行。
- 为 BASE 字体元数据建立有界缓存，避免每个字形重复解析。缓存键至少覆盖字体 Blob/collection face、Script、baseline tag 和影响基线值的 variation 状态；字体目录代际变化时失效。

不包含字体选择 UI、语言选择 UI、DOCX 导入导出、纵排、任意用户指定 BASE、重做文本 shaping，也不承诺不同系统字体下像素完全一致。

## 前置条件与待决策

024 的字体 shaping 和 026 的行距/分页规则已稳定；027 的 glyph 绘制链路能验证 Rust glyph placement 的实际结果。开始前需再次确认 Parley、`read-fonts` 与可用字体 fixture 的锁定版本和 API。

OpenType BASE 不规定 Caret 怎样选 dominant run。实现按架构文档中已记录的优先级执行；若测试表明该规则会使语言标记、混排或软换行产生不稳定结果，先修订架构再实现，不能私下采用“最长文本 run”或依赖系统当前字体顺序的策略。

## 实施步骤

1. 核对锁定 API 和 BASE tag/Script 映射，复用 `read-fonts`；确认字体 collection face 与 variation 数据的处理边界。
2. 把 dominant run 选择、BASE 坐标读取、glyph 偏移与行框计算拆成清晰职责，避免 Parley shaping、文档模型或 QML 保存重复格式状态。
3. 加入可控字体 fixture 和测试：Latin、Hani、Arab 混排，Common 标点，BASE 数据缺失/缺项/损坏，跨 Run combining mark，软换行，字体回退及大字号行框溢出。
4. 用实际 glyph placement 验证全角 `！` 不移动既有 Latin glyph；验证 Arabic 主脚本不被全局 Latin 锚点覆盖；验证不同脚本的 run 基线偏移和段落行推进彼此独立。
5. 检查缓存内存上限、代际失效及热路径，不在每次按键中重新解析完整字体表；记录基准规模和结果。
6. 更新页面排版架构和 027/本任务记录，完成候选实现后由 subagent 独立评审并复验。

## 预计改动

预计涉及 `crates/caret-layout/` 的基线解析、行度量与 Parley glyph placement，`crates/caret-engine/` 的页面 glyph 集成测试、固定字体 fixture、`ai-docs/architecture/editor/page-layout.md` 和本任务文件。若需要新的直接依赖，只添加与 `Cargo.lock` 同版本的 `read-fonts` workspace 依赖；不添加自制 OpenType 表解析器。

## 验收标准

- [ ] dominant run 选择和 Common/Inherited 归属有确定规则，且同一段落跨软换行稳定。
- [ ] 使用 OpenType BASE 数据对齐有对应表项的脚本；缺少表项、格式损坏或字体回退时走确定的度量回退。
- [ ] 输入/删除中英混排标点不会移动不相关的既有 glyph；Arabic 段落的参考脚本由有效语言规则决定，而非固定 Latin 字体锚点。
- [ ] 各行行框包含实际字体/对象所需范围，或准确标记其裁切/溢出；大字号和 fallback 不会无标记地侵入相邻行。
- [ ] 固定行距保持定义的行推进语义；脚本基线偏移不改变不相关行或后续分页位置。
- [ ] 字体 BASE 元数据缓存有界、可按字体代际失效，不按 glyph 或按键重复解析。
- [ ] 测试使用固定、可分发的字体数据；不依赖开发机安装的特定字体通过精确坐标断言。
- [ ] 独立评审已完成，发现的问题已处理并复验；旧的全局锚点实现、测试和文档说法已清理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --offline --locked -p caret-layout -p caret-engine` | 固定字体 fixture 的 BASE 对齐、缺失回退、行框和 glyph 坐标用例通过 | 未执行 |
| — | `cargo clippy --offline --locked -p caret-layout -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check` | Rust lint 和格式通过 | 未执行 |
| — | 定向 Qt 窗口 glyph placement 验收 | 排版 glyph 坐标与实际绘制使用同一偏移，画布无旧帧 | 未执行 |

## 风险与回退

系统字体对 BASE 的支持、版本和 variation 度量不同，需用固定 fixture 验证算法，再用系统字体做非像素精确的 smoke 检查。性能风险是每次布局重复读取、解析字体表；用面级缓存和明确上限处理。BASE 无法安全解释时回退到字体度量并保留溢出信息，不因容错而静默压缩行框。

## 决策与工作记录

- 2026-10-26：从 027 拆分脚本基线和大字号/字体回退的行框溢出问题；依赖 024、026、027 已满足，任务可开始。

## 完成摘要

未完成。依赖已满足，可开始实施。
