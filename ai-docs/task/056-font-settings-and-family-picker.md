# 056 — 字体设置与字体族选择器

- 状态：planned
- 依赖：024、025、039
- 优先级：P2
- 创建 / 更新：2026-10-30 / 2026-10-30

## 目标与背景

Caret 已通过 Fontique 读取并缓存系统/用户字体目录，也已建立按 Unicode Script 与 BCP 47 语言覆盖选择字体族的规则。025 已提供文档默认字体 API 和局部 `\font` 命令候选；本任务仍负责把 `Script` 与 `Script+Language` 字体目标接入 Settings 用户界面。字体目标、存储映射、回退链和层级规则以[字体选择规范](../architecture/editor/font-selection.md)为唯一权威；各平台系统回退映射见[平台字体映射目录](../architecture/editor/font-defaults/README.md)。

本任务补齐字体目录到用户可配置入口的链路，不重做字体枚举、字形覆盖判断或 fallback。字体偏好是排版设置：当前文档的值由 Rust 文档会话持有并写入 `.cdoc` 的 `globalSettings`；QML 只负责呈现候选、草稿和交互。应用级 Preferences 不得覆盖已打开文档的显式字体设置。

## 必读

- [字体塑形](024-text-shaping.md)、[格式命令](025-text-formatting.md)
- [多文字体系字体选择](../architecture/editor/font-selection.md)、[文字塑形](../architecture/editor/text-shaping.md)、[Settings 运行时](../architecture/core/runtime.md)
- [分层](../standards/layering.md)、[QML](../standards/qml.md)、[测试](../standards/testing.md)

## 范围与非目标

- 通过 Rust 服务暴露可用字体族/样式的稳定查询 DTO、当前目录代际和显式刷新能力；复用 024 的 Fontique `FontCatalog`，不得让 QML 扫描目录或维护第二份字体目录。
- 选择器只提供 `Script` 与 `Script+Language` 两类目标；所有 Unicode 单一脚本平级，`Latn` 置首。语言覆盖必须位于对应 Script 下。不得另设 Latin/CJK 一级分类；完整映射见[字体选择规范](../architecture/editor/font-selection.md)。
- 若本任务展示代码字体设置，它是独立的 `defaultCodeFontFamily` 文档字段，和数学字体一样不属于 Script 目标层级；只允许选择等宽字体，默认保留值为 `"<default>"`，由当前平台映射到系统等宽字体。字段与回退定义以[字体选择规范](../architecture/editor/font-selection.md)为准。
- 选择字体后修改当前文档 Rust `globalSettings`，保留请求族名。字体不存在、刷新后缺失或覆盖不足时显示可理解的状态；实际字体仍由既有 shaping/fallback 路径解析，不静默改写用户请求。
- 目录读取、刷新、候选过滤和偏好提交需有代际/失败语义；切换候选不得在每次按键时重新扫描磁盘或重复计算全量覆盖信息。
- 可在既有 Settings 对话框增加文档排版字体页/区域；QML 不成为文档字体设置的权威状态源。界面文案纳入所有已支持 locale 的翻译。
- 首版数学字体继续使用 011 约定的 STIX Two Math，不提供数学字体切换；局部 Font 环境与命令由 025 负责，本任务不重复实现。
- 不分发或安装字体，不复制字体文件到应用或 `.cdoc`，不承诺不同平台使用同名字体时度量完全一致。

## 前置条件与待决策

- 025 的字体字段、文档命令/API 和持久化行为先稳定；039 的 Settings UI/Rust 投影模式作为跨层范例。
- 实施前核验 Fontique 0.12 可公开提供的字体族/样式、覆盖查询及刷新 API。只在库 API 无法提供必要元数据时，才为缺失信息增加窄适配，不自行实现字体文件扫描或 Unicode 覆盖算法。
- 明确字体列表的显示名、同名字族区分和字体不可用状态；不能以字体族显示名作为不稳定的 face 唯一 ID。

## 实施步骤

1. 审核 024 的 FontCatalog 和 025 的字体设置接口，列出已有能力与缺口，确认目录代际、字体族/样式元数据及覆盖查询成本。
2. 在 Rust 定义目录查询/搜索 DTO 和刷新结果；字体设置通过投影并调用 025 已有文档字体命令/API 提交，验证候选后原子更新，失败不改变当前设置。
3. 在 Settings UI 展示字体族，按[字体选择规范](../architecture/editor/font-selection.md)中的 Script 与 Script+Language 目标选择；代码字体如纳入本页，则作为独立的等宽字体设置，不嵌入脚本目标树。搜索、键盘操作、无匹配、字体缺失和刷新状态均可观察。
4. 对目录结果和覆盖分类按 FontCatalog generation 缓存；确认刷新替换代际后旧候选不覆盖新状态，热路径不进行文件系统 I/O。
5. 增加 Rust 服务/持久化、FFI/Qt 投影及用户可见行为测试；验证中英混排、指定 Script 覆盖、缺失字体回退和重启恢复。
6. 同步所有 locale 翻译、架构文档与任务状态，按验证规范完成独立评审。

## 预计改动

复用 `crates/caret-layout` 字体目录与字体选择逻辑、`crates/caret-app` 文档设置服务、`crates/caret-ffi`、`native/viewmodels` 和既有 Settings QML 页面；必要时扩展 `ai-docs/architecture/editor/font-selection.md` 与相关测试。不得另建重复的字体扫描 crate 或 QML 字体目录。

## 验收标准

- [ ] Rust 返回的字体族目录来自 024 的 Fontique catalog；目录刷新有明确 generation，QML 不访问文件系统。
- [ ] 选择器能查询/选择所有可配置 `Script` 与 `Script+Language` 目标；目标与 `.cdoc` 字段的映射符合[字体选择规范](../architecture/editor/font-selection.md)，不另行定义 Latin/CJK 分类。
- [ ] 若本任务实现代码字体 UI，必须读写文档 `defaultCodeFontFamily` 并保证候选是等宽族；数学字体继续固定 STIX Two Math，不加入脚本选择器。
- [ ] 设置更新只影响当前文档 `globalSettings`，保存重开后保持；文档未选择字体时仍遵循系统默认与既有 fallback 规则。
- [ ] 不存在/无覆盖字体的请求值被保留并显示状态，实际 fallback 可查询；失败提交不产生部分设置或过期 UI 状态。
- [ ] 同一字体目录 generation 内的过滤/覆盖元数据可复用；输入与 shaping 热路径没有目录扫描，刷新后缓存正确失效。
- [ ] 界面在所有支持的 locale 下有完整翻译；字体名称作为系统数据不被错误翻译。
- [ ] 数学字体首版保持 STIX Two Math；局部 Font 环境仍由 025 管理。
- [ ] 行为测试覆盖选择、取消/失败、缺失字体、目录刷新、文档持久化及跨层状态；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 字体目录/设置服务测试 | 目录、刷新代际、提交失败与偏好保存语义正确 | 未执行 |
| — | Qt/QML 设置交互测试 | 搜索、分类、缺失状态与语言切换符合可观察行为 | 未执行 |
| — | 固定字体中英混排与 Script 覆盖测试 | 请求族/实际 fallback 可区分，缓存按代际失效 | 未执行 |

## 风险与回退

平台字体库暴露的名称和覆盖元数据可能不同；DTO 必须保留来源信息并允许同名族，不得假设跨平台稳定 face ID。刷新失败时保留旧 catalog 与现有设置；可用字体列表暂不可用时，已有文档仍按保存的请求名和系统 fallback 排版。

## 决策与工作记录

- 2026-10-30：从 024/025 核实现状。系统/用户字体目录读取、显式刷新、脚本/语言候选与 fallback 已由 024 完成；025 负责文档默认字体 API 与局部 `\font` 命令，本任务只补 Settings 中的文档默认字体选择 UI。
- 2026-10-30：建立本任务补齐字体设置查询与用户选择链路；复用 Fontique catalog，不重复扫描；文档字体状态由 Rust 持有并保存在文档全局设置中。

## 完成摘要

未完成。
