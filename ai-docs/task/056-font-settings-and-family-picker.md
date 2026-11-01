# 056 — 文档字体设置专用布局

- 状态：deferred
- 依赖：024、025、039
- 优先级：P2
- 创建 / 更新：2026-10-30 / 2026-10-30

## 目标与背景

Caret 已有 Rust 文档字体设置接口、字体目录和按 Unicode Script / BCP 47 Language 选择字体族的规则。本任务负责未来提供用户可见的文档默认字体设置界面；它与命令输入中的 `\font` / `\defaultfont` 候选流程分开，后者由[059](059-command-font-picker.md)负责。

字体设置界面的布局由用户单独设计。设计确定前本任务保持 deferred，不把字体选择页直接塞入现有通用 Settings 对话框，也不自行假设采用侧栏、弹窗或其他布局。字体目标、存储映射、回退链和层级规则以[字体选择规范](../architecture/editor/font-selection.md)为唯一权威；平台系统回退映射见[平台字体映射目录](../architecture/editor/font-defaults/README.md)。

字体偏好是文档排版设置：当前文档的值由 Rust 文档会话持有并写入 `.cdoc` 的 `globalSettings`；应用级 Preferences 不覆盖已打开文档的显式字体设置。QML 负责呈现用户最终确认的专用布局、候选及交互。

## 必读

- [字体塑形](024-text-shaping.md)、[格式命令](025-text-formatting.md)、[命令字体选择器](059-command-font-picker.md)
- [多文字体系字体选择](../architecture/editor/font-selection.md)、[文字塑形](../architecture/editor/text-shaping.md)、[Settings 运行时](../architecture/core/runtime.md)
- [分层](../standards/layering.md)、[QML](../standards/qml.md)、[测试](../standards/testing.md)

## 范围与非目标

- 等待字体设置界面的独立布局设计完成后，再实现专用 UI；不并入现有通用 Settings 对话框的页面布局。
- 复用 Rust 文档字体设置 API 和 024 的 Fontique `FontCatalog`；不得由 QML 扫描字体目录或维护第二份字体选择规则。
- 选择目标只使用 `Script` 与 `Script+Language` 两类；Unicode Script 平级，`Latn` 置首，语言覆盖位于对应 Script 下。完整映射见[字体选择规范](../architecture/editor/font-selection.md)。
- 代码字体 `defaultCodeFontFamily` 是独立的等宽字体文档字段，不嵌入 Script 目标树；统一默认值 `"<default>"` 使用内置 JetBrains Mono。数学字段也使用该默认标记并解析为内置 STIX Two Math，不提供数学字体切换 UI。标记含义与跨层实现以[字体选择规范](../architecture/editor/font-selection.md)为准。
- 选择字体后经 Rust 既有文档设置 API 提交并写入当前文档；保存重开保持请求族名。字体缺失或覆盖不足时保留请求并展示状态，实际字形继续由既有 shaping/fallback 路径解析。
- 目录刷新、查询、过滤和覆盖信息复用既有 generation/caching 机制；热路径不扫描磁盘。
- 不重新实现字体枚举、Unicode 覆盖判断、fallback 或命令候选弹窗；代码与数学默认字体由公共资源目录内置，不由本任务复制字体文件，不承诺跨平台度量一致。

## 前置条件与待决策

- 用户提供或确认专用字体设置界面的布局设计后，将任务从 deferred 调整为 ready，并据此补齐交互细节与验收截图/行为。
- 025 的文档字段、命令/API 和持久化行为稳定；039 的 Rust 设置投影及提交模式可供参考，但不复制其通用 Settings 页面布局。
- 实施前复核 Fontique 0.12 当前可公开提供的字体族/样式、覆盖查询和刷新 API；只在 API 确有缺口时增加窄适配。
- 明确字体显示名、同名字族区分、不可用状态、取消/失败提交和刷新中交互；不能将显示名用作不稳定的 face 唯一 ID。

## 实施步骤

1. 布局设计确定后，核对字体目录与文档设置 API，列明已存在能力和 UI 所需 DTO 缺口。
2. 复用 Rust 服务获取字体候选和当前文档设置；候选选择通过既有文档 API 原子提交，失败不改写当前值。
3. 按已确认的独立布局实现字体目标与候选交互，覆盖搜索、键盘操作、无匹配、缺失字体、刷新状态和取消。
4. 校验目录 generation、缓存失效和旧异步结果丢弃；输入与 shaping 路径不得增加目录 I/O。
5. 补齐所有支持 locale 的界面翻译和可观察行为测试，验证文档保存、重开及字体回退。
6. 更新架构文档与任务状态，按验证规范完成独立评审并复验。

## 预计改动

复用 `crates/caret-layout` 字体目录与字体选择逻辑、`crates/caret-app` 文档设置服务、`crates/caret-ffi`、`native/viewmodels` 和 QML 字体设置 UI。具体界面目录与组件边界须服从后续布局设计；不得另建重复字体扫描 crate 或 QML 字体目录。

## 验收标准

- [ ] 专用字体设置界面严格按确认后的布局设计实现，不占用现有通用 Settings 对话框页面。
- [ ] 字体目录来自 Rust/Fontique；QML 不访问文件系统、不复制字体目录或覆盖规则。
- [ ] `Script` 与 `Script+Language` 的目标层级、持久字段和优先级符合[字体选择规范](../architecture/editor/font-selection.md)，没有 Latin/CJK 一级分类。
- [ ] 文档字体设置通过 Rust 当前文档 API 更新，保存重开保持；应用级 Preferences 不覆盖文档显式字体值。
- [ ] 缺失/无覆盖字体的请求值保留且状态可见；实际 fallback 可查询；取消或提交失败不会产生部分更新或过期 UI 状态。
- [ ] 代码字体仅接受等宽族且 `"<default>"` 解析为 JetBrains Mono；数学字段的同一标记解析为 STIX Two Math。
- [ ] 目录 generation 内的查询可复用，刷新后正确失效；输入与 shaping 热路径没有目录扫描。
- [ ] 所有支持 locale 的 UI 文案有翻译；字体名称作为系统数据不被翻译。
- [ ] 行为测试覆盖选择、取消/失败、缺失字体、目录刷新、持久化和跨层状态；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 字体目录/文档设置测试 | 查询、刷新代际、失败原子性和持久化正确 | 未执行 |
| — | Qt/QML 字体设置行为测试 | 交互符合确认布局，locale 切换和缺失状态可观察 | 未执行 |
| — | 固定字体中英混排与 Script 覆盖测试 | 请求族与实际 fallback 可区分，缓存正确失效 | 未执行 |

## 风险与回退

布局设计尚未确定，提前实现容易把临时布局固化为产品约束，因此本任务延期。实现阶段若平台字体元数据不同，DTO 需保留来源信息并允许同名字族；刷新失败保留旧 catalog 与当前设置。每项 UI 可独立回退，不改写 `.cdoc` 字段语义。

## 决策与工作记录

- 2026-10-30：将命令中的 `\font` / `\defaultfont` 弹窗交互与 Settings 中的文档默认字体界面拆分；本任务只负责后者。
- 2026-10-30：字体设置采用独立布局；用户完成布局设计前保持 deferred，不直接加到现有通用 Settings 对话框。

## 完成摘要

未完成。
