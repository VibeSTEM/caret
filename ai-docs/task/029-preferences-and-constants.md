# 029 — Preferences 服务与跨层常量归属

- 状态：done
- 依赖：003、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-27

## 目标与背景

Settings 是应用设置的总入口；Preferences 是用户偏好分类。持久化偏好必须由 Rust 管理，不能由 QML、C++ 或 Theme DSL 各存一份。Rust Settings 服务已存在，原桌面设置窗口尚未接入；`DocumentCanvas.caretBlinkEnabled` 是视图输入属性，由 029 接上 Preferences 投影。

本任务把跨层数值按语义归位，并接通用户偏好链路。目标不是把所有数字搬进一个常量文件，而是让可配置值只有一个权威来源，算法和单位换算常量留在其所属领域，简单数学关系与协议值保持就地可读。

## 必读

- [运行时](../architecture/core/runtime.md)、[光标与选区职责](../architecture/interface/caret.md)、[界面外壳](../architecture/interface/ui-shell.md)
- [分层](../standards/layering.md)、[注释](../standards/comments.md)、[测试](../standards/testing.md)

## 范围与非目标

- 将用户偏好 schema、默认值、校验、提交与持久化统一在 Rust `caret-app` 的 Preferences 服务；Settings 界面为总入口，Preferences 为用户偏好分类。
- 通过 ViewModel 发布偏好快照并提交候选；QML 只显示和提交意图，C++ 只接收绘制/行为所需的当前值。
- 将光标闪烁开关加入 Rust 偏好；关闭时常亮，开启时跟随 Qt 平台周期，系统周期为 0 时常亮。
- 盘点 QML/C++/Rust 的生产数值，识别可配置值重复、没有语义名称的产品阈值和尺寸；优先消除重复来源，并把有业务意义且会调整的量移到合适的 Rust Preferences 或 `ThemeService`。
- UI 外观尺寸/颜色由主题负责；算法阈值、文档单位换算、协议字段及格式码由所属模块定义并命名；0/1 哨兵、数组下标、循环边界、比例公式和测试夹具不为消除数字而机械提取。
- 不建立全局 `MagicNumbers`/`Constants` 大杂烩，不改变数值语义，不把 GUI 外观或偏好写入 `.cdoc`。
- 将本次审计认定的职责拆分落地：Rust Settings/Session/FFI、Qt ViewModel/Adapter/LayoutViewModel、Canvas 子模块按已归属的状态与功能组织；同步架构和注释规范，不按文件行数机械拆分。

## 前置条件与待决策

003 的 Rust 设置服务与主题服务已存在，027 提供画布闪烁输入属性。设置 schema 扩展须兼容已有用户配置；遇到损坏配置时保留既有隔离和内存回退行为。

## 实施步骤

1. 审计 QML、C++ 和 Rust 生产代码的数值字面量，按 Preferences、Theme、领域算法、协议/哨兵、测试数据分类并记录重复来源。
2. 依据 Settings 总入口与 Preferences 分类明确 Rust API 和 Qt ViewModel 边界，迁移现有偏好服务命名或职责，保留磁盘配置兼容。
3. 接通 Preferences 设置 UI，并将光标闪烁值绑定到 `DocumentCanvas.caretBlinkEnabled`。
4. 修正重复的默认值/范围来源；具名常量带单位并靠近唯一消费者，不相关算法常量不跨模块搬迁。
5. 为旧配置迁移、候选校验失败、持久化失败、设置切换和值传播补行为测试；独立评审完整 diff 后复验。

## 预计改动

`crates/caret-app/src/`、`native/viewmodels/`、`qml/Dialogs/SettingsDialog.qml`、`qml/App.qml`、`qml/PageSurface.qml`、`native/canvas/`、主题/设置规范与相关 Qt/Rust 测试。最终改动以审计结果为准。

## 验收标准

- [x] Rust 是所有持久化用户偏好的唯一权威来源；ViewModel、QML 与 C++ 不保存副本。
- [x] Settings 总入口内有 Preferences 分类；用户能切换光标闪烁，值在重启后保留。
- [x] 开关关闭时焦点内插入点常亮；开启时遵循系统周期；系统周期为 0 时常亮；失焦始终隐藏。
- [x] 现有用户配置可加载；新增字段缺省使用有文档的默认值，损坏/无效设置不会覆盖最后有效快照。
- [x] 重复的可配置值只有一个权威来源；主题值留在主题服务，领域算法常量留在领域模块并具有含义清楚的名称和单位。
- [x] 缩放百分比与倍率，以及 pt/centipixels/logical px 边界换算，全部通过 `DocumentUnits` API；调用点没有手写或复制转换公式。
- [x] 不存在为消除 0/1、下标、数学比例和测试样例而新增的无意义常量层。
- [x] Rust、C++、QML 单元/集成测试覆盖设置 schema、转换、持久化和实时生效；独立评审无未解决阻塞项。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-27 | `cargo test --locked -p caret-app` | 默认值、兼容迁移、校验、只读恢复和原子文件替换正确 | 60 项通过 |
| 2026-10-27 | `cargo test --locked -p caret-engine -p caret-ffi` | Session/FFI 拆分保持命令、布局与栅格边界行为 | engine 55 项、FFI 4 项通过；DTO 调整后 FFI 4 项复验通过 |
| 2026-10-27 | `cargo test --locked -p caret-tests --test document` | 共用原子存储后 `.cdoc` 保存与失败保留行为不变 | 18 项通过 |
| 2026-10-27 | `cargo clippy --locked -p caret-app -p caret-engine -p caret-ffi --all-targets -- -D warnings` | 手写代码不依赖生成桥的 lint 豁免 | 通过 |
| 2026-10-27 | `cargo fmt --all --check`、workspace Clippy、`cargo test --locked --workspace --exclude caret-tests` | 全 Rust 生产模块格式、lint 与测试通过 | 通过；native 聚合由 Qt 验证单独执行 |
| 2026-10-27 | `cargo test --locked -p caret-tests --bin caret-tests` | 迁移后的路径选择器与测试调度保持正确 | 45 项通过 |
| 2026-10-27 | `cmake --build target/native/debug --parallel 4` | 桌面入口、Adapters、ViewModels、Canvas 与 Shell 可一起构建 | 通过 |
| 2026-10-27 | `cargo test --locked --workspace` | Rust、测试选择器和 native 聚合全部通过 | 通过；Rust/runner 全部通过，CTest 4/4（SDK、i18n、ShellLoad、QML 格式） |
| 2026-10-27 | ViewModel include-cleaner、CMake format/lint、Canvas 翻译单元语法检查 | 模块移动保持 include 和分层正确 | ViewModel 10/10；CMake 检查通过；Canvas 6 个翻译单元通过语法检查 |
| 2026-10-27 | `cargo lint` | Rust、CMake、分层、QML、clang-tidy、include-cleaner、cppcheck 全量静态检查通过 | 通过；25 个自有 C++ 翻译单元检查通过 |
| 2026-10-27 | 独立 final review 与常量审计收口 | 重复来源消除，剩余数字有明确语义或保留理由 | 通过；无未解决阻塞项，持久化警告通过 Qt 日志可见 |

## 风险与回退

设置边界调整可能影响现有配置 JSON 和主题/语言选择。迁移需先验证完整候选，再原子提交；失败时继续使用最后有效快照。常量迁移不改行为，单项可独立回退。

## 决策与工作记录

- 2026-10-26：Settings 作为总入口，用户偏好归入 Preferences；Preferences 与文档内容、界面主题 token 分离。
- 2026-10-26：用户设置全部由 Rust 管理。QML 只呈现和发送意图，ViewModel 只转换和桥接，C++ 视图组件不持久化偏好。
- 2026-10-26：常量整理按语义归属，禁止全局收纳数字的无差别常量文件。
- 2026-10-26：初审发现 `App.qml` 初始窗口尺寸 `1120×760` 与 Rust 偏好默认值 `1280×720` 不一致；缩放条 `0.5–2.0`、步进 `0.1` 与 `DocumentCanvas` 的 `0.1–8.0` 限制来源分散。QML 多数控件尺寸已通过 `Theme` 管理；颜色通道、数组下标、0/1 哨兵和单位换算等数学值不应机械提取。
- 2026-10-27：Qt 设置对话框的草稿与关闭流程由专用 `SettingsDialogController` 持有 `QStateMachine`，状态为 `Closed → Editing → Applying → Closed/Failed`。Rust `SettingsRuntime` 决定候选校验、持久化结果和可写状态；Qt 状态机只管理临时表单草稿与界面流程，不复制持久化权威值，也不推断提交成功。
- 2026-10-27：画布可见页范围直接从 `DocumentCanvas.documentToContent()` 获取实际缩放坐标，避免把浮点缩放折算为整数百分比后再计算页面几何。
- 2026-10-27：缩放百分比与倍率仅允许经 `DocumentUnits` 正反向 API 转换；调用点禁止内联比例公式或复制换算常量。新增单位转换应归入拥有该单位语义的领域工具，并在评审时检索是否有绕过工具的手写换算。
- 2026-10-27：Rust `settings` 按 `model`、`service`、`runtime`、`storage` 和 Preferences/Theme/Language 分类；主题 schema 归 `theme/schema.rs`。`SettingsRuntime` 使用 `Ready`、`ReadOnly`、`Unavailable` 显式状态，文件访问失败保留只读有效快照，已隔离的无效文件允许从默认值继续提交。
- 2026-10-27：设置与文档写入共用应用内 `storage/atomic_file.rs`。原子替换是提交点；替换之后的目录同步错误报告为持久性警告，不回退内存造成磁盘/内存分歧。不可变主题目录使用共享所有权，单项偏好事务不深拷贝全部主题 token。
- 2026-10-27：`caret-engine` 的 Session 按文本编辑、导航和 IME composition 拆到 `editing/session/`，`Session::run` 保持唯一命令调度入口。`caret-ffi` 的手写文档/设置适配移到独立模块，生成桥的 unsafe/lint 豁免不再覆盖手写转发；glyph 栅格请求改为具名 DTO。

## 完成摘要

Settings 服务与 UI 状态边界已落地，相关 Rust/Qt/native 文件按职责拆分，标准和结构文档已同步。缩放与单位边界统一由 `DocumentUnits` API 转换；独立 final review、全量数值审计和仓库 lint 均已通过。
