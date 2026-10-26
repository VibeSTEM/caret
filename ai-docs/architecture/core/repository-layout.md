# 仓库结构

更新日期：2026-10-27。先列当前仓库目录，再单列尚未创建的编辑器领域 crate，避免把目标结构误读为现状。没有消费者之前不建空 crate。依赖方向以[分层规范](../../standards/layering.md)为准。

```text
当前仓库
├── crates/
│   ├── caret-app/
│   │   └── src/
│   │       ├── settings/
│   │       │   ├── model.rs
│   │       │   ├── service.rs
│   │       │   ├── runtime.rs
│   │       │   ├── storage.rs
│   │       │   ├── preferences.rs
│   │       │   ├── language.rs
│   │       │   └── theme/{mod.rs,schema.rs}
│   │       └── storage/atomic_file.rs
│   ├── caret-build/
│   ├── caret-dsl-core/
│   ├── caret-dslc/
│   ├── caret-doc/
│   ├── caret-ffi/
│   │   └── src/{lib.rs,document.rs,settings.rs,tests.rs}
│   ├── caret-engine/
│   │   └── src/editing/session/{mod.rs,text_edit.rs,navigation.rs,composition.rs}
│   ├── caret-foundation/
│   ├── caret-layout/
│   └── launcher/
├── native/
│   ├── adapters/             # 平台字体、路径、窗口 chrome、图标
│   ├── app/
│   ├── canvas/
│   │   └── document_canvas/{caret,geometry,glyph_frames,input,scene_graph}.cpp
│   ├── cmake/
│   ├── i18n/
│   └── viewmodels/
│       ├── document/         # 布局快照、命令桥、glyph frame 适配
│       ├── settings/         # Rust runtime bridge、Preferences 投影、设置对话框状态机
│       └── theme/
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

规划中的 crate
├── caret-structure/     # 只读结构索引
└── caret-cli/           # 应用命令行
```

crate 的职责和允许依赖以[分层规范](../../standards/layering.md)为唯一来源；运行时职责见[运行时](runtime.md)。`caret-engine` 已创建并通过 `Session::run` 提供编辑命令入口；`caret-cli` 已列入任务 014，尚未创建。

## 模块拆分原则

- 按职责、状态所有权和变化原因拆分；文件行数本身不是拆分条件。拆分后仍由同一消费者构建的文件可以留在同一个 Cargo crate 或 CMake target 中，不为目录层级自动新建库。
- Rust 模块由 `mod.rs` 汇总稳定入口，领域模型、服务协调、运行时生命周期、存储适配和分类规则各归其模块。共享低层机制仅在出现真实重复消费者后提取到所属应用层。
- Qt ViewModel 按 Settings、Theme、Document 分类；Runtime bridge 单独持有对应 Rust runtime，分类 ViewModel 只投影自己的领域属性。平台 API 适配放在 `native/adapters/`，画布和 ViewModel 不互相依赖。
- 大型会话、画布和布局桥按文本编辑/导航/IME、绘制/几何/光标/输入、快照/命令/glyph 适配等职责拆分；外部命令入口保持集中，子模块不各自建立并行状态机或第二套数据权威。

`caret-app::settings` 是应用设置总协调模块；分类服务分别负责 Preferences、Theme 和 Language，`SettingsRuntime` 负责加载、恢复、可写状态与保存结果。`storage/atomic_file.rs` 是应用层内部的原子写入工具，调用者仍拥有自身的编码、校验和隔离策略。

`native/viewmodels/` 按领域分类；`native/adapters/` 只依赖 Qt 与平台 API。Canvas 子目录按绘制、几何、光标和输入拆分，仍由单一 `caret_canvas` 目标构建。设置窗口由 `SettingsDialogController` 管理 Qt 交互状态；QML 不复刻控制器状态或 Rust 持久化设置。

下载的工具在 `.toolchain/`。编译产物在 `target/`。生成的桥接头和 QM 留在构建树。

QML 组件清单见[QML 规范](../../standards/qml.md)。编辑器页面（文稿视图、状态栏、命令补全）是 Caret 自己的装配，不从参考仓库的工艺面板搬来。
