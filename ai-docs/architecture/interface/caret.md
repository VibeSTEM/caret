# 文稿光标与选区

本文定义文稿光标与选区的职责边界。模型和编辑行为属于 Rust；Qt 仅将其位置和布局几何呈现出来。

## 状态来源

- `caret-engine` 持有光标、锚点、活动端、逻辑环境亲和性和文档位置；这些状态不由 QML 或画布复制维护。
- `caret-layout` 根据文档与字体生成视觉 caret 候选和 selection 几何，包括换行、分页及双向文本的映射。
- 布局结果使用 `(sessionId, documentGeneration)` 标识输入版本；替换或克隆 Session 会生成新 ID，成功文档变更递增 generation，选区和 IME 预编辑不改变文档代际。
- Qt 画布接收同一 generation 的布局结果，绘制 caret 和选区；拖动期间的预览可在 C++ 本地暂存，释放时提交最终位置。
- 文字插入、删除、格式继承、逻辑亲和性和位置映射由 Rust 命令处理。像素坐标与 Rust UTF-8 位置的转换只在适配边界发生。

## 外观

- 纸张、页边框、光标、选区和选区文字等 Canvas 颜色由 `qml/Themes/` 中的主题值定义，经 `Themes/Theme.qml` 暴露并传给画布；C++ 不硬编码产品颜色，Rust 文档也不保存界面颜色。
- 主题 token 使用独立的 `color-caret`、`color-selection` 和 `color-selection-text`。新增 token 时同步更新默认、浅色和深色主题及主题校验测试。光标宽度以整数百分之一逻辑像素保存在 `caret-width-centipixels`，由画布在 scene graph 几何边界转为逻辑像素。pt 与逻辑像素的换算由 `DocumentUnits` 统一处理；跨 QML 传整数百分之一逻辑像素，在消费 Qt 几何 API 时才转为浮点值。
- 插入点仅在画布获得活动焦点且 Rust 已提交插入位置时显示；失焦时隐藏。获得焦点或位置变化时立即显示。Settings 的 Preferences 分类应提供光标闪烁开关，由 Rust `PreferencesService` 持有；开启时按 Qt 平台周期闪烁，关闭时常亮，平台周期设为 0 时也常亮。ViewModel 将值传给 `DocumentCanvas.caretBlinkEnabled`；QML 只呈现和提交设置，画布不持久化偏好。当前仓库尚无 `PreferencesService` 和这条设置桥接，后续集成时应复用 Rust 偏好服务，不再并行新增 QML/C++ 状态。键盘编辑和 IME 状态仍由后续集成接入，不能由画布自行维护第二份文档状态。
- 选区透明度和高对比度替代色仍待设计；这些是视图参数，不属于 `.cdoc`。
- 环境背景高亮是独立的临时视图叠加，不与文本选区或 caret 颜色合并。

## 绘制和性能

- 布局结果提供 caret 和 selection 的页面内几何；画布使用页面坐标与 zoom 映射绘制，不重新排版文本。Caret 几何来自 Rust 命中结果，`DocumentCanvas` 负责 scene graph 绘制并裁切到纸张范围。
- 拖选预览只更新画布局部几何；布局未改变时不触发 Rust 往返或整页排版。
- 跨页选区仍是一份 Rust 逻辑范围，视图按可见页面裁剪几何。
- QML 负责主题绑定和焦点/无障碍语义；不在 QML 中重建 Rust 的选区或键盘移动规则。

## 相关任务

- [022：文本编辑、选择与 IME](../../task/022-text-editing-and-ime.md)
- [027：页面画布、命中与布局快照](../../task/027-canvas-and-hit-testing.md)

具体粗细和各状态颜色仍待界面设计确认；本页不预设具体视觉数值。
