# QML 组件与对话框

更新日期：2026-10-29。本文说明当前 QML 分层、组件边界和验证要求。Qt 对象见 [Qt](qt.md)，图标见[图标](icons.md)，主题源文件见 [DSL](dsl.md)。

## 分层

```text
Theme.qml
    ↓ token
Atoms → Composites → 页面与对话框
```

`Theme.qml` 是只读 token 门面，不属于组件层。Atom 封装单一交互控件或可复用视觉元素，不依赖业务服务。Composite 组合多个控件形成完整交互部件，不从任意父对象读取隐式状态。页面和对话框负责组装控件；业务状态与持久化仍通过 ViewModel 进入 Rust。

组件只有在当前有调用方，或已由明确的近期任务确定会复用时才注册到 QML 模块。删除组件时同步删除 QML 模块注册、专属主题 token、资源和本文清单。不要为单个内部 Item 单独建 Atom；只被一个组件使用的细节优先内聚在该组件内。

## 原子组件

目录：`qml/Components/Atoms/`。

| 文件 | 职责 |
| --- | --- |
| `ThemedLabel.qml` | 主题文字与换行 |
| `ThemedButton.qml` | 常规主题按钮 |
| `ThemedToolButton.qml` | 工具按钮，支持图标、状态、下拉指示和辅助文案 |
| `ThemedTextField.qml` | 文本输入与校验外观 |
| `ThemedNumericField.qml` | 带数值校验的文本输入 |
| `ThemedCheckBox.qml` | 复选框；指示器实现内聚在控件中 |
| `ThemedSwitch.qml` | 紧凑开关 |
| `ThemedRadioButton.qml` | 单选框 |
| `ThemedComboBox.qml` | 主题下拉框 |
| `CommandBarTabButton.qml` | 命令栏页签，字重变化不改变相邻项位置 |
| `TitleBarPillButton.qml` | 标题栏胶囊按钮 |
| `ThemedIcon.qml` | 模块内 SVG 图标 |

## 组合组件

目录：`qml/Components/Composites/`。

| 文件 | 职责 |
| --- | --- |
| `DocumentModeSelector.qml` | 编辑、审阅、查看模式选择 |
| `ShareButton.qml` | 分享操作入口与菜单 |
| `DialogWindow.qml` | 模态窗口与居中 |
| `DialogFrame.qml` | 对话框标题与内容装配 |
| `DialogTitleBar.qml` | 对话框标题栏、拖动和关闭信号 |
| `DialogButtonRow.qml` | 对话框操作按钮行 |

组合控件应有明确的当前调用方；无调用方的组合组件和其专属 token 不留在模块中。不要保留未使用的 Ribbon、后台菜单、页签条或通用面板框架作为预先扩展点。

## 页面与对话框

页面文件位于 `qml/` 根目录或 `qml/Dialogs/`。对话框可以持有草稿和校验反馈；保存与业务命令经 ViewModel 进入 Rust。使用 `Qt.FramelessWindowHint` 时，标题栏通过平台移动接口拖动窗口。关闭按钮只发信号；打开条件和业务命令由具体对话框处理。

## 主题与写法

- `qml/Themes/Theme.qml` 是唯一的 QML token 门面；token 由 Rust schema 校验、C++ ViewModel 发布。
- 主题 DSL、Rust schema 与 QML 门面只保留当前实现使用的 token。
- 间距、字号、圆角、图标和控件高度来自 token。调用方覆盖时仍使用 token 或布局计算。
- 组件文件用 `PascalCase.qml`，id、属性和信号用 `camelCase`；注释按中文注释规范编写。
- 用属性绑定表达状态。命令式赋值前先确认不会拆掉绑定。
- `qt_add_qml_module` 与 QML 目录保持一致；不依赖开发机的 import path。
- 应用入口使用 `QQuickStyle::setStyle("Basic")`。自绘控件不运行在原生 Controls 样式上，测试环境使用相同设置。
- 布局使用逻辑像素，不在 QML 里乘 device pixel ratio。
- 可见文案用 `qsTr()` 或 `qsTranslate()`，英文作为源文本。

## 验证

使用锁定版本的 Qt `qmllint` 和格式工具检查实际模块。组件验证默认 token、禁用/聚焦/交互状态以及主题切换后的绑定。对话框验证打开、取消、确认和关闭时没有残留草稿。打包后的 QML 与资源加载单独验证。
