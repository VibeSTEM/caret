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
- 主题 token 使用独立的 `color-caret`、`color-selection` 和 `color-selection-text`。新增 token 时同步更新默认、浅色和深色主题及主题校验测试。当前已实现 `color-caret` 和 `caret-width-px`，宽度以逻辑像素传给画布。
- 闪烁周期、焦点/失焦表现、选区透明度和高对比度替代色仍待设计；这些是视图参数，不属于 `.cdoc`。
- 环境背景高亮是独立的临时视图叠加，不与文本选区或 caret 颜色合并。

## 绘制和性能

- 布局结果提供 caret 和 selection 的页面内几何；画布使用页面坐标与 zoom 映射绘制，不重新排版文本。Caret 几何来自 Rust 命中结果，`DocumentCanvas` 负责 scene graph 绘制并裁切到纸张范围。
- 拖选预览只更新画布局部几何；布局未改变时不触发 Rust 往返或整页排版。
- 跨页选区仍是一份 Rust 逻辑范围，视图按可见页面裁剪几何。
- QML 负责主题绑定和焦点/无障碍语义；不在 QML 中重建 Rust 的选区或键盘移动规则。

## 相关任务

- [022：文本编辑、选择与 IME](../../task/022-text-editing-and-ime.md)
- [027：页面画布、命中与布局快照](../../task/027-canvas-and-hit-testing.md)

具体粗细、闪烁及各状态颜色仍待界面设计确认；本页不预设具体视觉数值。
