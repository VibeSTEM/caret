# 仓库结构

更新日期：2026-10-25。先列当前仓库目录，再单列尚未创建的编辑器领域 crate，避免把目标结构误读为现状。没有消费者之前不建空 crate。依赖方向以[分层规范](../../standards/layering.md)为准。

```text
当前仓库
├── crates/
│   ├── caret-app/
│   ├── caret-build/
│   ├── caret-dsl-core/
│   ├── caret-dslc/
│   ├── caret-doc/
│   ├── caret-ffi/
│   ├── caret-foundation/
│   └── launcher/
├── native/
│   ├── app/
│   ├── cmake/
│   └── i18n/
├── qml/
│   ├── Components/
│   ├── Dialogs/
│   ├── Themes/
│   └── icons/
└── tests/
    ├── cpp/
    ├── fixtures/
    ├── integration/
    ├── performance/
    ├── rust/
    └── src/

规划中的编辑器领域 crate
├── caret-layout/        # 纯排版
├── caret-structure/     # 只读结构索引
└── caret-engine/        # Session、命令、.cdoc
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

crate 的职责和允许依赖以[分层规范](../../standards/layering.md)为唯一来源；运行时职责见[运行时](runtime.md)。`caret-cli` 已列入任务 014，规划中但尚未创建。

下载的工具在 `.toolchain/`。编译产物在 `target/`。生成的桥接头和 QM 留在构建树。

QML 组件清单见[QML 规范](../../standards/qml.md)。编辑器页面（文稿视图、状态栏、命令补全）是 Caret 自己的装配，不从参考仓库的工艺面板搬来。
