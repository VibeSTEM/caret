# 仓库结构

更新日期：2026-10-24。下面是目标树。没有消费者之前不建空 crate。依赖方向以[分层规范](../../standards/layering.md)为准。

```text
crates/
├── caret-build/         # 工具供给，安装到 .toolchain/
├── caret-foundation/    # 崩溃信号
├── caret-dsl-core/      # .pa
├── caret-dslc/
├── caret-doc/           # 文档模型，无 I/O 策略
├── caret-layout/        # 纯排版
├── caret-structure/     # 只读结构索引
├── caret-engine/        # Session、命令、.cdoc
├── caret-app/           # 路径、设置、语言、主题；调用 engine
├── caret-ffi/           # DTO 与转发
└── launcher/            # 调度 CMake 并启动窗口
native/
├── adapters/            # Qt 路径、线程、窗口几何
├── canvas/              # 绘制、命中、插入点、IME
└── viewmodels/
qml/
├── Components/
├── Dialogs/
├── Themes/
└── App.qml
resources/i18n/
tests/
```

`caret-doc`、`caret-layout`、`caret-structure` 不依赖引擎，也不知道 Qt。`caret-engine` 依赖这三者。`caret-app` 依赖引擎和 DSL，向引擎传入已经解析的路径。`caret-ffi` 只依赖 `caret-app`。界面 crate 不依赖领域 crate。

下载的工具在 `.toolchain/`。编译产物在 `target/`。生成的桥接头和 QM 留在构建树。

QML 组件清单见[QML 规范](../../standards/qml.md)。编辑器页面（文稿视图、状态栏、命令补全）是 Caret 自己的装配，不从参考仓库的工艺面板搬来。
