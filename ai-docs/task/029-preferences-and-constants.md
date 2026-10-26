# 029 — Preferences 服务与跨层常量归属

- 状态：planned
- 依赖：003、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-26

## 目标与背景

Settings 是应用设置的总入口；Preferences 是用户偏好分类。持久化偏好必须由 Rust 管理，不能由 QML、C++ 或 Theme DSL 各存一份。当前 `caret-app::SettingsService` 有 Rust schema、默认值、校验和文件读写，但桌面设置界面尚未通过 ViewModel 接入；`DocumentCanvas.caretBlinkEnabled` 目前只是视图输入属性。

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

- [ ] Rust 是所有持久化用户偏好的唯一权威来源；ViewModel、QML 与 C++ 不保存副本。
- [ ] Settings 总入口内有 Preferences 分类；用户能切换光标闪烁，值在重启后保留。
- [ ] 开关关闭时焦点内插入点常亮；开启时遵循系统周期；系统周期为 0 时常亮；失焦始终隐藏。
- [ ] 现有用户配置可加载；新增字段缺省使用有文档的默认值，损坏/无效设置不会覆盖最后有效快照。
- [ ] 重复的可配置值只有一个权威来源；主题值留在主题服务，领域算法常量留在领域模块并具有含义清楚的名称和单位。
- [ ] 不存在为消除 0/1、下标、数学比例和测试样例而新增的无意义常量层。
- [ ] Rust、C++、QML 单元/集成测试覆盖设置 schema、转换、持久化和实时生效；独立评审无未解决阻塞项。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust PreferencesService 单元测试 | 默认值、兼容迁移、校验和文件往返正确 | 未执行 |
| — | Qt ViewModel/QML/native 测试 | UI 切换值进入 Rust 并即时生效 | 未执行 |
| — | 三语言常量审计与独立评审 | 重复来源消除，剩余数字有明确语义或保留理由 | 未执行 |

## 风险与回退

设置边界调整可能影响现有配置 JSON 和主题/语言选择。迁移需先验证完整候选，再原子提交；失败时继续使用最后有效快照。常量迁移不改行为，单项可独立回退。

## 决策与工作记录

- 2026-10-26：Settings 作为总入口，用户偏好归入 Preferences；Preferences 与文档内容、界面主题 token 分离。
- 2026-10-26：用户设置全部由 Rust 管理。QML 只呈现和发送意图，ViewModel 只转换和桥接，C++ 视图组件不持久化偏好。
- 2026-10-26：常量整理按语义归属，禁止全局收纳数字的无差别常量文件。
- 2026-10-26：初审发现 `App.qml` 初始窗口尺寸 `1120×760` 与 Rust 偏好默认值 `1280×720` 不一致；缩放条 `0.5–2.0`、步进 `0.1` 与 `DocumentCanvas` 的 `0.1–8.0` 限制来源分散。QML 多数控件尺寸已通过 `Theme` 管理；颜色通道、数组下标、0/1 哨兵和单位换算等数学值不应机械提取。

## 完成摘要

未完成。
