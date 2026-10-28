# 038 — 多窗口文档会话与全局操作协调

- 状态：planned
- 依赖：037
- 优先级：P2
- 创建 / 更新：2026-10-28 / 2026-10-28

## 目标与背景

当前应用仅创建一个主窗口；`LayoutViewModel` 和 Rust `DocumentRuntime` 通过 QML/C++ singleton 与进程级 bridge 暴露。该结构无法支持独立文档窗口，也无法兑现 019 规定的跨窗口 AutoSave 开启流程。本任务在 037 完成单窗口 AutoSave 后，建立每窗口独立的文档会话，同时保留进程级 Settings/Preferences 服务。

## 必读

- [AutoSave 策略](019-autosave-design.md)、[AutoSave 实现](037-autosave-implementation.md)
- [运行时](../architecture/core/runtime.md)、[界面外壳](../architecture/interface/ui-shell.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 每个文档窗口拥有独立 `LayoutViewModel`、Rust `DocumentRuntime`、布局/选区投影、打开与保存队列、AutoSave deadline 和恢复会话。
- 应用级 Settings/Preferences 仍由一个 Rust 服务持有，变更投影到所有窗口。
- AutoSave 全局开关开启时，Rust 协调器按活动窗口优先顺序串行收集未命名会话的路径结果；提交偏好前校验 operation generation 和每个会话 generation。
- 打开、新建、关闭、窗口销毁、开关变更或另一个开启流程启动时，使受影响的旧事务失效；完成的另存结果保留。
- 不包含标签页、协同编辑、单文档多窗口并发写保护策略以外的新协作能力，也不重做窗口视觉设计。

## 前置条件与待决策

037 的单窗口 AutoSave、保存冲突、恢复和关闭语义通过实现验收。窗口创建入口、macOS 文档打开回调与其他平台的窗口复用策略在实施前确认并写入界面架构。

## 实施步骤

1. 梳理 QML singleton、进程级 bridge、布局快照及异步回调中隐含的单窗口假设。
2. 设计应用级 Settings 服务与窗口级文档运行时的生命周期和所有权边界。
3. 建立窗口创建/销毁流程，迁移 QML 与 C++ 命令到窗口绑定的 ViewModel/runtime。
4. 在 Rust 应用服务实现全局 AutoSave 开启事务、活动窗口排序、串行路径请求与过期 generation 拒绝。
5. 增加双窗口测试，覆盖同名临时会话、不同编辑/保存状态、窗口关闭、路径选择取消/失败、乱序回调和 stale generation。
6. 按验证规范完成多平台验证、完整 diff 独立评审并更新架构与索引。

## 预计改动

`native/viewmodels/document/`、`native/viewmodels/document_runtime_bridge.*`、`native/app/`、`qml/App.qml` 与窗口创建组件、`crates/caret-ffi/src/document.rs` 和 `crates/caret-app/` AutoSave 协调服务；具体边界按所有权分析确定。更新运行时和界面架构文档。

## 验收标准

- [ ] 同时打开两个窗口时，每个窗口的文档、选区、布局、保存状态、撤销历史、AutoSave deadline 和恢复绑定完全隔离。
- [ ] 任一窗口的编辑、打开、保存、恢复、关闭不会改动其他窗口的文档状态；关闭一个窗口不销毁其他窗口正在使用的运行时。
- [ ] 全局 Settings/Preferences 只有一个 Rust 权威服务；偏好变更被所有窗口正确通知，不复制第二份已提交状态。
- [ ] 跨窗口开启 AutoSave 时只出现一个路径对话框；按活动窗口优先串行处理未命名文档，全部成功后才持久化开启。
- [ ] 任一窗口在操作期间打开/新建/关闭文档、切换开关或启动新事务时，旧回调不能重新启用偏好或修改新会话；已完成的另存路径保留。
- [ ] 恢复项跨进程保护和单窗口事务在多个窗口同时恢复、更新、另存、丢弃时不会共享可变 envelope 或丢失仍在编辑的内容。
- [ ] 自动化测试覆盖两个以上窗口及路径选择取消、失败、过期 generation、保存回调乱序和单窗口销毁；用户可观察行为与 019 一致。
- [ ] macOS、Windows、Linux 窗口生命周期验证通过；职责边界、异步所有权和资源清理经独立 subagent 完整评审。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待实施后填写 | 双窗口生命周期、跨窗口设置传播和 AutoSave 事务正确 | 未执行 |

## 风险与回退

当前全局 singleton 被 QML 组件广泛引用。迁移时若一次性替换所有引用会扩大回归面；应先定义稳定的窗口级注入接口，并让 ViewModel/runtime 的销毁与异步任务取消有明确所有者。若尚未完成所有权迁移，不得用共享 singleton 临时模拟多个窗口。

## 决策与工作记录

- 2026-10-28：根据 037 实施验收，将跨窗口文档运行时与全局 AutoSave 开启事务拆为独立任务；037 仅对当前单窗口实现负责。

## 完成摘要

未开始。完成后记录窗口级运行时所有权、跨窗口 AutoSave 事务和三平台验证证据。
