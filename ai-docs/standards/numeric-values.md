# 数值与单位

更新日期：2026-10-27。数值的单位、精度、合法范围和权威来源必须明确；持久化值和跨语言边界采用稳定表示，只有计算或绘制确有需要时才转换表示。

## 整数定点值

需要固定精度、跨 Rust/Qt 传递或写入文档的有限小数，优先使用有明确单位的整数定点值。字段名或邻近类型要表达单位和缩放，例如：

| 值 | 存储 | 例子 | 浮点转换边界 |
| --- | --- | --- | --- |
| 正文字号 | `u16` 百分之一 pt（centipoints） | `1200` 表示 `12 pt` | Rust 排版调用 Parley 时 |
| 光标宽度 | 主题 token 与 Qt 属性使用整数百分之一逻辑像素（centipixels） | `125` 表示 `1.25 logical px` | C++ 构造 scene graph 矩形时 |

缩放因子必须由单位定义固定下来，不能由消费者猜测。Rust schema 校验范围，Qt 适配器在转换前做必要的范围检查。整数乘除使用检查运算或安全的中间类型；转换为 `f32`、`f64` 或 `qreal` 前验证范围，浮点输入还必须拒绝 NaN 和无穷值。

同一单位换算只定义在一个领域工具中。Rust 文档字号与段落距离的 centipoints ↔ pt 转换由 `caret_doc::DocumentUnits` 管理；Qt 画布中的 pt → logical px、字号 centipoints → centipixels 与 centipixels → logical px 转换由 QML `DocumentUnits` 门面及其 C++ 实现管理。跨 QML 传递整数字号和整数百分之一逻辑像素，只有消费 Qt 几何 API 时再转换为浮点像素。QML 中字号增减仍使用 centipoints 整数，不先转 pt 浮点。单位归属、转换入口和边界见[文档单位架构](../architecture/editor/document-units.md)。

缩放百分比与浮点倍率必须通过 `DocumentUnits.percentToFactor()` / `DocumentUnits.factorToPercent()` 转换；C++ 内部调用对应的 `document_units::percent_to_factor()`。调用点不得内联 `/100`、`*100`、`0.01` 等换算，也不得新增第二套 helper。`CanvasPolicy` 只负责允许的缩放范围和步进，不复制单位换算。pt、centipixels 与 logical px 也必须复用 `DocumentUnits` 中的对应转换。

后续新增单位边界时，应由拥有该单位语义的领域工具提供有名称的正反向转换 API；UI、ViewModel、绘制和 FFI 调用点只调用 API，不手写比例公式或复制换算常量。`DocumentUnits.centipointsToLogicalCentipixels()` 是字号进入 QML 绘制时的转换入口。评审数值变更时，要搜索调用点中的换算表达式，确认没有绕过既有工具。

## 何时使用浮点

整数定点不是所有几何值的强制格式。页面尺寸、边距和排版计算若需要保留更细的物理精度，可以使用带明确物理单位的 `f64`/`f32`；读取和运算时必须验证有限性、正负约束和边界。页面 pt、逻辑 px 与设备 px 是不同坐标空间，转换只能在明确的适配边界进行，DPR 不写入文档属性。

外部库若要求浮点值，在调用边界转换；不得为了省一次转换而把浮点扩散到持久化 DTO、跨语言接口或缓存键。浮点测试必须说明单位和容差。

UI 可以在布局和绘制过程中持有带明确单位的 `real` 倍率，例如画布的 logical scale；它应由整数百分比经 `DocumentUnits.percentToFactor()` 派生，默认百分比由缩放策略集中提供。不要在 QML 属性或控件中另写 `1` 作为 100% 的第二个来源。绘制 API 要求 `real` 时，在该边界使用派生倍率。

## 默认值和可调参数

- 每个可调值只有一个权威来源。文档排版默认值放在 `crates/caret-doc/resources/layout-themes/`，GUI 外观 token 放在 `qml/Themes/*.pa` 并由 Rust schema 校验；QML `Theme.qml` 只提供只读门面。
- QML/C++ 绘制代码不另存一份设置默认值。C++ 只保留用于几何算法且不属于用户设置的命名常量；Rust 负责文档与偏好语义。
- 对应精度、范围、单位或默认值改变时，同步更新 schema、资源、适配器和测试。改动已保存文档语义时，按 `.cdoc` 的 `requiredFeatures` 声明能力。
- 用名称表达可配置值和算法常量，不在调用点散落无解释的比例、阈值、时间或尺寸数字。

## 验收

覆盖默认值、最小/最大值、无效值、单位转换和边界舍入。固定点至少测试一个非整数显示值（例如 `125 -> 1.25`），确认整数在配置与跨层接口中不被提前转为浮点。显示和绘制像素测试再验证最终几何。
