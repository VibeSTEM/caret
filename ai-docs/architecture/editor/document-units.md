# 文档单位与跨层转换

更新日期：2026-11-01。本文是 Caret 文档定点单位与 Rust/Qt 转换边界的架构权威；数值合法性和通用检查见[数值规范](../../standards/numeric-values.md)。

## 权威归属

持久化文档使用稳定整数单位。`caret-doc::DocumentUnits` 是 Rust 文档和排版侧对 centipoints 与 points 的唯一转换入口；文档 schema 的单位含义和可表示范围也由此处定义。布局、公式后端和引擎只调用该 API，不复制比例因子或重新推导字号上限。

Qt/QML 使用另一个同名 `DocumentUnits` 门面，它消费 Rust/FFI 提供的单位值，负责 logical px、centipixels、pt 和 zoom factor 等 Qt 几何 API 边界转换。C++ `document_units` 是 QML 门面的实现。它不成为文档字段单位或 Rust 排版转换的第二来源。

## 单位表

| 值 | 文档 / FFI 表示 | 转换权威 | 消费边界 |
| --- | --- | --- | --- |
| 字号 | `u16` centipoints | Rust `DocumentUnits::centipoints_to_points` 与 `points_to_font_size_centipoints` | `caret-layout` 交给 Parley；字号回读到文档位置 DTO 时再转为 `u16` |
| 段前、段后、固定行距 | `u32` centipoints | Rust `DocumentUnits::centipoints_to_points` | Rust 行框和分页计算 |
| 页面尺寸与边距 | 有明确字段名的物理 pt `f64` | `caret-doc::PageGeometry` | Rust 页面布局；Qt 画布消费页面快照 |
| 光标宽度、主题尺寸 | 整数百分之一 logical px（centipixels） | Qt `DocumentUnits` | 构造 QSG 矩形时转为 `qreal` logical px |
| 页面 pt 与 logical px | 文档保持物理 pt | Qt `DocumentUnits`，包含 zoom | Canvas/QSG 几何 API |
| zoom | 整数百分比；QML 控件内部使用派生倍率 | `DocumentUnits.percentToFactor` / `factorToPercent`，C++ 对应 `document_units` | Qt zoom 与输入控件 |

## 数值范围

文档字号可表示为 `1..=u16::MAX` centipoints。公式后端接受 pt 浮点字号前，必须验证有限、正值且不超过 `DocumentUnits::max_font_size_points()`；布局字号回写到文档 `u16` 字段时，反向转换拒绝 NaN、无穷、非正数和超出 `u16` 范围的值，并按 centipoint 最近值取整。其他 Rust 布局 API 按各自输入契约校验，不因本文而默认具有统一的字号上限检查。

段落行距和间距使用 `u32` centipoints，进入 `f32` 行框前调用同一 Rust 换算入口。页面尺寸等原生物理几何本来就是 pt，不执行 centipoints 换算。转换为 qreal、逻辑像素或设备像素前，消费方仍检查有限性和范围；DPR 不持久化。

## 依赖方向

持久化单位属于 `caret-doc`，所以 `caret-math` 与 `caret-layout` 依赖其转换 API。`caret-doc` 不依赖排版、Qt、FFI 或会话状态。Qt 单位实现只位于画布适配层；Rust 和 Qt 的同名门面共享同一单位契约，但不互相调用。

禁止在调用点内联 `/100`、`*100`、`0.01` 等 centipoints 比例；zoom 和 logical px 转换也只能通过 Qt `DocumentUnits`。增加单位或改变精度时，同步更新本文、numeric-values、权威工具、跨层 DTO 和边界测试。
