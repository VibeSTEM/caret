# 033 — 多语言共享行距网格与选区几何

- 状态：done
- 依赖：024、026、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-30

## 目标与背景

修正同字号多语言内容因字体回退度量不同而改变行框、选区高度或后续段落位置的问题。用户实测 Microsoft Word 中不同语言的行距保持一致；Caret 的目标是同一基础样式下使用稳定行距网格，不让同字号 fallback 字体的全局 ascent/descent 偶然扩大行框。

本任务已完成的是共享行距网格及其行框/选区映射，不是 OpenType BASE 表解析或跨脚本基线对齐。原任务中尚未实现的 BASE dominant run、字体表读取、字形墨迹溢出诊断、固定字体 fixtures 和性能测量已移交 [055](055-opentype-base-script-alignment.md)。

## 必读

- [页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)
- [段落排版任务](026-paragraph-layout.md)、[验证与评审](../standards/validation-and-review.md)
- [Microsoft Word 行距官方说明](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)（仅用于官方文档明确规定的语义；不据此推断 Word 未公开的内部字体/基线算法）

## 已完成范围

- 文档/段落基础样式字体和字号用于解析共享行距网格，测量采用稳定的 Latin 样本 `M`；结果按字体代际、文本设置和塑形样式缓存。
- 每个 Parley 视觉行映射到统一的行框和主基线；同字号 fallback run 不抬高网格。明确更大字号仍可按行距规则增加该行所需高度。
- 空段落、非空段落、软换行行使用相同行距网格；选区背景按共享行框几何绘制，而非随各字形 ink bounds 改变高度。
- Microsoft Word 官方文档中的行距类型和段前/段后语义继续按 026 的架构记录；本任务不声称已复刻 Word 的未公开实现。

## 验收标准

- [x] 同一基础样式下，段落语言标签和软换行不改变共享行距网格。
- [x] 同字号 Latin 与 CJK 内容的行框及选区背景使用相同高度；空段落与非空段落的网格一致。
- [x] 更大字号的 ascent/descent 需求仍按行距模式参与行框计算，不被共享网格裁掉。
- [x] 段落间距仍使用独立 pt 值，不由 fallback 字体度量隐式增加。
- [x] 未将 Parley 的行基线映射误称为 OpenType BASE 脚本基线；BASE 后续实现由 055 承接。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-30 | `cargo test --offline --locked -p caret-layout -p caret-engine -p caret-doc` | 行网格、选区几何、行距和段落布局行为通过 | 通过：`caret-engine` 142 项、`caret-layout` 84 项、`caret-doc` 29 项；包含 `line_grid_stays_stable_across_language_and_soft_wrapped_lines`、`selection_background_follows_shared_line_box_not_font_ink_bounds` 和更大字体混合度量测试 |
| 2026-10-30 | `cargo clippy --offline --locked -p caret-layout -p caret-engine -p caret-doc --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | lint、格式及差异检查通过 | 通过；Criterion target 编译包含在 all-targets 检查中 |
| 2026-10-30 | `DocumentLayoutService::resolve_line_grid_metrics` 与 Parley 行映射代码核对 | 记录实际网格来源，不把网格实现写成 BASE | 通过；网格由基础样式文本设置和 Latin 样本度量确定，`baseline_adjustments_pt` 是共享行框与 Parley 单行几何之间的映射，不是 BASE 表解析结果 |

## 风险与限制

当前验证证明的是仓库实现及其测试输入下的共享行框行为，不是对任意系统字体、Word 所有版本或屏幕像素结果的完全一致承诺。测试目前使用系统字体目录，固定、可分发字体 fixture 与 BASE 表兼容测试属于 055。

## 决策与工作记录

- 2026-10-26：从 027 拆出多语言基线与字号/字体回退行框问题。
- 2026-10-30：行距网格与未来 BASE 脚本基线对齐分开处理。用户关注同字号多语言选区和段落起点一致；网格改为由基础样式字体与字号确定，避免同字号 fallback 度量扩大行框。
- 2026-10-30：审查源码后确认没有 OpenType BASE 表读取实现。将原 033 过宽的 BASE、dominant run、ink overflow、固定字体和缓存验收范围迁移至 055；本任务只验收已落地的共享行距网格。

## 完成摘要

共享行距网格、Parley 行到统一行框的几何映射及选区高度测试已完成。OpenType BASE 脚本基线并未实现，作为独立任务 055 保留，不能将 033 的 done 状态解释为 BASE 支持已完成。
