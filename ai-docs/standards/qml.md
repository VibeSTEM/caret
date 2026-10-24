# QML 组件与对话框

更新日期：2026-10-24。适用于规划中的 `qml/`。Qt 对象见 [Qt](qt.md)，图标见[图标](icons.md)，主题源文件见 [DSL](dsl.md)。

组件库从 panta 的 `qml/Components` 和通用对话框移植。移植时把模块名 `Panta.Shell` 改成 `Caret.Shell`，并改掉资源 URL 里的前缀。工艺面板和只服务于几何、网格、结果视图的对话框不在清单里。

## 分层

```text
Theme.qml
    ↓ token
Atoms → Composites → 通用对话框
    ↓ 意图
页面装配（按 Caret 的编辑器另行设计）
```

Theme 不计入组件层级。原子组件不接业务服务。组合组件不读取任意父对象。对话框可以持有草稿和校验反馈，保存仍经 ViewModel 进入 Rust。

页面装配、Ribbon 里的具体工具页，以及分析、网格、材料面板不随组件库移植。编辑器外壳另做。

## 原子组件

目录：`qml/Components/Atoms/`。

| 文件 | 职责 |
| --- | --- |
| `ThemedLabel.qml` | 主题文字 |
| `ThemedButton.qml` | 主题按钮 |
| `ThemedToolButton.qml` | 图标、后缀、下拉指示、选中和禁用 |
| `ChromeToolButton.qml` | 标题栏工具按钮 |
| `ThemedTextField.qml` | 文本输入和校验态 |
| `ThemedNumberField.qml` | 数值输入 |
| `ThemedCheckBox.qml` | 复选框 |
| `ThemedSwitch.qml` | 紧凑开关。默认关闭，轨道为灰色 |
| `ThemedRadioButton.qml` | 单选框 |
| `ThemedComboBox.qml` | 下拉框 |
| `RibbonComboBox.qml` | 命令条里的下拉框 |
| `CheckIndicator.qml` | 勾选指示 |
| `MenuTabButton.qml` | 菜单页签按钮 |
| `PanelSurface.qml` | 面板表面 |
| `ThemedIcon.qml` | 模块内 SVG |

这些控件封装 Qt Quick Controls，保留焦点、键盘和可访问性。不按内部 Item 的数量再拆。

## 组合组件

目录：`qml/Components/Composites/`。

| 文件 | 职责 |
| --- | --- |
| `DialogWindow.qml` | 模态和居中 |
| `DialogFrame.qml` | 标题与内容装配 |
| `DialogTitleBar.qml` | 无边框拖动和关闭信号 |
| `DialogButtonRow.qml` | 确认和取消 |
| `FormSection.qml` | 表单分组 |
| `DocumentTabBar.qml` | 文档页签 |
| `PanelTabBar.qml` | 等宽分段切换 |
| `PaneCloseButton.qml` | 面板关闭 |
| `HorizontalToolStrip.qml` | 横向滚动和焦点显露 |
| `ToolGroup.qml` | 工具分组 |
| `RibbonGroup.qml` | 命令分组 |
| `RibbonTile.qml` | 图标加文字的工具块 |
| `RibbonContent.qml` | 按模型渲染分组，只发出动作键 |
| `InformationPanel.qml` | 信息区 |
| `ApplicationBackstageCommand.qml` | 后台菜单命令 |

`RibbonContent` 不解释命令含义。页签里放哪些工具由编辑器页面决定。

## 对话框

| 文件 | 处理 |
| --- | --- |
| `SettingsDialog.qml` | 移植。设置项随后按 Caret 的 schema 裁剪 |
| `LanguageSettingsPage.qml` | 移植。locale 列表来自 Rust |
| `NewProjectDialog.qml` | 移植窗口、校验和按钮行。字段改成 Caret 的新建文档，不保留来源工程的工艺选项 |
| `AnalysisSequenceDialog.qml`、`FillProcessSettingsDialog.qml`、`GateLocationSettingsDialog.qml`、`HoldingProfileDialog.qml`、`MaterialSelectionDialog.qml`、`ImportDialog.qml` | 不移植 |

对话框使用 `Qt.FramelessWindowHint` 时，标题栏通过平台移动接口拖动窗口。关闭按钮只发信号。打开条件和业务命令留在具体对话框，不写进 `DialogWindow`。

## 主题与写法

- `qml/Themes/Theme.qml` 是唯一的 QML token 门面，属性只读，绑定 C++ 发布的快照。`gui-default.pa`、`light.pa`、`dark.pa` 和 Rust schema 使用同一组键，只保留界面和已移植控件读到的项。
- 间距、字号、圆角、图标和控件高度来自 token。调用方覆盖时仍用 token 或布局计算。
- 组件文件用 `PascalCase.qml`，id、属性和信号用 `camelCase`。
- 用属性绑定表达状态。命令式赋值前先确认不会拆掉绑定。
- `qt_add_qml_module` 放在和 QML 目录一致的位置。不靠开发机的 import path 勉强能加载。
- 应用入口用 `QQuickStyle::setStyle("Basic")`。自绘控件不跑在原生 Controls 样式上。测试环境使用同一设置。
- 布局使用逻辑像素，不在 QML 里乘 device pixel ratio。
- 可见文案用 `qsTr()` 或 `qsTranslate()`，英文作为源文本。

窗口最小尺寸在编辑器界面里定。移植外壳时不照抄来源仓库的工作区下限。

## 验证

用锁定 Qt 的 qmllint 和格式工具检查实际模块。每个移植的控件验证默认 token、覆盖值，以及主题切换后绑定仍在。对话框验证打开、取消、确认和关闭时不留下过期草稿。打包后的资源加载单独验证。
