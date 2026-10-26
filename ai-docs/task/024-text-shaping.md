# 024 — 字体回退与文字塑形

- 状态：done
- 依赖：022、023
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-26

## 目标与背景

实现真实 shaping、字体覆盖/回退和双向文字映射，生成供行布局和命中使用的 glyph/cluster 数据。

## 必读

- [文字塑形](../architecture/editor/text-shaping.md)、[格式语义](../architecture/editor/text-formatting.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [分层](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

实现系统字体目录读取、脚本/方向分段、字体回退、字重/字形解析、shaping、Unicode 换行机会和双向映射。目录包含当前用户和系统已安装字体，支持显式刷新并缓存结果，不为每次编辑重新扫描。接收调用方按 020/025 格式优先级解析好的基础 `ShapingStyle`，再应用 Paragraph/Run 语言和 Font/Bold/Italic 环境；其他直接格式、全局覆盖和主题解析属于 025 格式解析器。请求字体与实际替代字体分开呈现。用户安装字体可以使用，但 Caret 不复制到安装包或文档；分发字体时检查许可证。

不包含未获许可字体、Word 分页一致承诺和完整 OpenType 控件。

## 前置条件与待决策

022 的文本位置、023 的版心几何接口稳定。设计选定实际使用的 shaping API。

采用 Parley Engine 0.12 的段落分析与 shaping API，字体发现/回退使用 fontique 0.12；行断使用 ICU4X 2.3.0 的 Unicode 17 UAX #14，grapheme 使用 `unicode-segmentation` 1.13.3，Parley 的 bidi 与 character properties 使用随锁定版本嵌入的 Unicode 17 数据。官方完整 UCD 测试集已加入仓库。Parley Engine 目前逐行 reshaping 仍在开发中；Caret 的布局适配不得假设它已处理任意断点后的重新塑形。

## 实施步骤

1. 评估 Parley Engine/HarfRust shaping、fontique 字体目录及统一 Unicode 数据版本；核实 Qt/平台字体 API 的用户级字体发现、刷新和实际字体数据访问能力。
2. 通过 Unicode 17 官方 GraphemeBreakTest、BidiCharacterTest 和 LineBreakTest 后锁定依赖。实现缓存字体目录、段落样式展开、cluster 映射和回退，不自行实现标准算法。
3. 建立中英混排、泰语词典断行、组合字符、emoji、Indic、阿拉伯 RTL 和缺失字体样例。
4. 独立评审并复验本机可运行的平台构建；macOS/Windows 字体后端由 PR CI smoke 验证。

## 预计改动

caret-layout 字体/shaping 模块、Windows 系统字体刷新适配、缓存键和跨平台测试。

## 验收标准

- [x] 段落逻辑文本展平 Run/行内环境，保留 UTF-8 字节到 character/cluster/glyph 的映射；字体树本身不被改写。
- [x] 覆盖中英、Run 语言范围内的泰语词典断行、RTL、组合字符、Indic、emoji/ZWJ、西里尔字母、希腊字母与缺失字体 fallback。
- [x] Unicode 算法及数据精确锁定为 Unicode 17；官方 grapheme、bidi-character、line-break 完整 corpus 均通过。
- [x] 缺失字体保留请求名，同时报告塑形实际使用的字体族。
- [x] fontique 使用平台字体后端读取系统及用户族；显式 refresh 构建并替换目录代际；输入/塑形路径不扫描文件系统。Windows 刷新额外读取标准 Fonts 目录。
- [x] SystemUi generic family 作为未指定请求族时的主族；Bold/Italic 优先同族字形，缺少字形时由 Fontique 合成或按脚本 fallback；文档环境语义保留。
- [x] 段落塑形接收已解析的基础样式；格式全局值、主题命名样式与 Run/Paragraph 直接格式优先级由 025 解析并传入。
- [x] 未加入应用字体资源；字体目录代际作为布局缓存失效输入。
- [x] 独立只读 subagent review；P1/P2 发现已修复并复验。
- [x] Linux/Windows 系统字体目录和刷新 smoke；本机另有 macOS aarch64/CoreText 验证。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | `cargo test --locked -p caret-layout` | Unicode 17 官方 UCD corpus 与 shaping/fallback fixtures 通过 | 15 项通过；三份完整 corpus、Run 分片一致、0.5/12.25 pt、Cyrillic/Greek、emoji/ZWJ 非零 glyph、词典字素边界与 Run 语言范围泰语均通过 |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout -p caret-engine`、Clippy 与变更格式检查 | 相邻同语言 Run/环境边界不影响词典/中文 tailoring 断点 | 通过；caret-layout 69 项、caret-engine 54 项；新增泰语词典断行与中文行首禁则分片回归 |
| 2026-10-25 | `cargo clippy --locked -p caret-layout --all-targets -- -D warnings` | 新 crate 无 lint 告警 | 通过 |
| 2026-10-25 | macOS 字体 smoke | 请求字体和 fallback 行为一致 | 本机 aarch64/CoreText 测试通过；Windows/Linux 待平台 CI |
| 2026-10-26 | [Nightly run 40212431385](https://github.com/VibeSTEM/caret/actions/runs/40212431385)，Linux/Windows workspace tests | 两个平台均读取系统字体并显式刷新目录成功 | `caret-layout::tests::explicit_font_refresh_replaces_catalog_generation` 在 Ubuntu 和 Windows 2022 均通过；该用例断言系统族非空、刷新代际递增且目录替换成功。整轮 Nightly 12/12 job 通过 |

## 风险与回退

字体版本变化会改变度量与分页。固定测试字体 fixture，不承诺跨平台默认字体分页一致。

## 决策与工作记录

- 2026-10-25：从原 008 整体布局拆出文字塑形。
- 2026-10-25：采用 Parley Engine 0.12/HarfRust、fontique 0.12、ICU4X 2.3.0 与 Unicode 17；因 Parley Engine 行 reshaping 尚未完成，Caret 的规范断行机会由独立 UAX #14 接口提供。
- 2026-10-25：实现 `caret-layout`、平台字体目录显式刷新、嵌套 Run/Font/Bold/Italic 样式展平和 Unicode 17 全量 grapheme/bidi-character/line-break conformance。
- 2026-10-25：根据独立评审合并相同有效样式 Run、按 LB1 映射 SA、缓存字体别名和样式索引、支持按 Run 语言范围的词典断行、emoji 专用字体回退，并拒绝不完整塑形结果。词典断点限定完整字素边界，语言范围匹配以二分查找避免断点数乘范围数增长。
- 2026-10-26：修复词典/中文行断 tailoring 在同语言 Run 边界漏应用的问题；相邻的同 primary dictionary language 范围及连续中文范围合并，新增泰语词典断点和中文行首标点跨 Run 回归。
- 2026-10-25：字号 API 使用整数百分之一 pt（界面仍显示/输入 pt），支持 0.5 pt 与更细的 0.01 pt 精度；Parley 与布局输出保持 pt，视图边界才转换 logical px。
- 2026-10-26：Nightly run 40212431385 在 Ubuntu 和 Windows 2022 实际运行并通过系统字体目录刷新测试；本任务所有验收项完成，状态改为 done。

## 完成摘要

本机实现与 Unicode 17 conformance 完成，macOS/CoreText smoke 通过；Nightly 的 Ubuntu 和 Windows 2022 workspace tests 也通过系统字体目录和显式刷新 smoke。Parley Engine 尚未提供任意断点后的逐行 reshaping，后续行布局必须在 Atom/ShapedCluster 安全边界断行。Windows 对非标准路径注册或删除系统字体的即时检测不在 fontique 0.12 适配保证范围。
