# 058 — QML 主题令牌与固定常量分层

- 状态：ready
- 依赖：029、039
- 优先级：P2
- 创建 / 更新：2026-10-30 / 2026-10-30

## 目标与背景

当前 QML 中的颜色、字号、间距和控件尺寸有些经 `Theme.qml` 读取 Rust 主题令牌及 `.pa` 主题文件，有些直接写在组件中。需要建立清晰、可审计的归属规则，并整理现有值，避免相同用途出现多份来源，也避免为了消除字面量而把所有值塞进全局常量。

本任务确定并落实三种归属：用户可定制的界面外观使用 Rust 校验的 GUI Theme token，经 `.pa` 提供主题值并由 `Theme.qml` 暴露；跨组件共享且不允许主题覆盖的固定 UI 规则集中在独立的 `ThemeConstants.qml`；只服务单个组件的固定布局策略留在组件内。由内容、窗口可用空间、字体度量或平台指标推导的尺寸继续使用响应式绑定或对应系统 API。

## 必读

- [QML 规范](../standards/qml.md)、[数值与单位](../standards/numeric-values.md)、[分层规范](../standards/layering.md)、[验证与评审](../standards/validation-and-review.md)
- [界面外壳](../architecture/interface/ui-shell.md)、[Caret 与选区](../architecture/interface/caret.md)、[画布高亮](../architecture/interface/canvas-highlighting.md)
- [Preferences 与常量归属](029-preferences-and-constants.md)、[Settings 中的主题与界面语言](039-settings-theme-language-ui.md)

## 范围与非目标

- 审核生产 QML 中的颜色、字体、间距、圆角、尺寸、延迟和其他固定参数，为每项确定唯一归属；记录有意保留在组件内的局部参数及原因。
- 用户可能希望随配色/外观主题调整的值归入 GUI Theme：更新 Rust 主题 schema、默认/浅色/深色 `.pa` 文件及 `Theme.qml` 门面，保持 Rust 为值校验与主题快照的权威来源。
- 新增 `qml/Themes/ThemeConstants.qml`（QML singleton）作为跨组件共享、固定且不受用户主题覆盖的 UI 参数入口；在 QML 模块中注册并验证加载。
- 单组件使用的固定布局策略留在该组件；按内容、可用空间、字体度量或平台指标变化的量使用绑定/系统数据，不伪装成固定常量或主题 token。
- 不建立通用 `MagicNumbers`/`Constants` 大杂烩，不为消除数字字面量而提取哨兵值、索引、循环边界或简单算式；不将持久化偏好、文档排版值或领域算法参数放入 QML Theme。
- 不改变视觉行为、数值单位、用户主题兼容性和 UI 响应式行为；不扩展 Settings UI。

## 前置条件与待决策

029 已规定跨层常量按语义归属，039 已接通 Rust Theme 服务与设置 UI。开始时先核对现有 GUI Theme schema、所有内置 `.pa` 文件及其 QML 消费点；不得把 `Theme.qml` 当作第二份默认值存储。

分类时使用以下判据：

1. 用户切换主题时合理地希望改变的外观值，是 Theme token。
2. 多个组件必须遵守同一固定契约且不应由主题改变的值，才是 `ThemeConstants.qml` 候选。
3. 只表达某个组件自身策略的固定值，留在该组件并使用有意义的属性名。
4. 随运行时空间/内容/字体/平台变化的量，应由绑定或权威系统数据计算，不进入固定值集合。

如果某个尺寸既可视又可能影响可用性，实施前需依据现有产品约定决定是否主题化，并在变更记录中说明，不以“它是数字”作为归类理由。

## 实施步骤

1. 清点生产 QML 的固定颜色和数值，排除测试夹具、单位换算和动态绑定；标记重复值、现有 Theme token、组件局部值及候选共享常量。
2. 为每项记录归属、单位、权威来源、消费者和是否应随主题变化；合并重复来源，拒绝无使用者或仅为形式而建的抽象。
3. 按分类迁移值。GUI Theme token 的默认值只存在于 Rust schema/主题资源链路，QML `Theme.qml` 只读投影；共享固定常量在 `ThemeConstants.qml` 定义；组件局部属性保持就地。
4. 更新 QML 构建注册和相关架构/数值文档，说明三类值的判断规则及扩展流程。
5. 增加可观察的主题切换与常量稳定性验证；检查默认、浅色、深色主题 schema 一致、QML singleton 可加载、布局仍根据窗口/内容/翻译变化。
6. 依验证规范完成独立评审，复查 QML 调用点、Rust schema、主题资源、构建注册和重复值，再更新任务状态。

## 预计改动

`qml/Themes/Theme.qml`、新增 `qml/Themes/ThemeConstants.qml`、`qml/Themes/*.pa`、Rust GUI Theme schema/默认值、`qml/CMakeLists.txt`、相关 QML/主题测试，以及 [数值与单位](../standards/numeric-values.md)、[QML 规范](../standards/qml.md)或界面架构文档。实际范围以清点结果为准。

## 验收标准

- [ ] 生产 QML 固定颜色/数值均已分类；每个应提取的值有明确归属，局部保留值有原因，无未解释的重复来源。
- [ ] 用户主题应能调整的界面外观值由 Rust Theme schema 和 `.pa` 主题资源提供，并由 `Theme.qml` 只读暴露；切换内置主题能更新对应界面表现。
- [ ] 跨组件共享且固定的 UI 参数由 `ThemeConstants.qml` 单一提供，QML singleton 可构建、加载；主题切换不会改写这些值。
- [ ] 单组件专属参数仍归组件所有；动态尺寸仍跟随内容、可用空间、字体度量和平台数据，不引入固定窗口/弹窗尺寸回归。
- [ ] Preferences、文档排版默认值、算法阈值、协议值、测试夹具和单位转换没有被误迁移到 UI 常量或 GUI Theme。
- [ ] 所有内置 `.pa` 文件与 Rust schema 保持一致；主题加载/校验、QML 加载和关键响应式行为测试通过。
- [ ] 文档给出新增 Theme token、共享 UI 常量、组件局部参数的判定规则；独立评审无未解决问题。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 生产 QML 字面量清点及归属表 | 覆盖所有相关值并能解释保留项 | 未执行 |
| — | 主题 schema/资源验证与主题切换行为测试 | 内置主题一致且可定制 token 即时生效 | 未执行 |
| — | QML singleton 加载、QML 格式与布局行为测试 | 常量可用，窗口/内容变化时布局仍自适应 | 未执行 |
| — | 独立 final review | 分层、唯一来源、重复实现及文档规则通过 | 未执行 |

## 风险与回退

错误分类可能把本应可主题化的尺寸固化，或把产品布局约束暴露成主题设置。迁移应保留现有值和视觉行为；每组 token/常量独立变更并经主题切换及布局行为验证，发现不适合的归属时可逐项回退，不引入格式迁移。

## 决策与工作记录

- 2026-10-30：建立 Theme token、跨组件固定 UI 常量、组件局部参数和动态布局值的四分归属；共享固定参数候选入口为 `qml/Themes/ThemeConstants.qml`，不创建无差别全局常量表。

## 完成摘要

未完成。
