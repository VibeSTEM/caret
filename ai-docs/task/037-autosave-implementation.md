# 037 — AutoSave 实现

- 状态：ready
- 依赖：018、019、029
- 优先级：P2
- 创建 / 更新：2026-10-28 / 2026-10-28

## 目标与背景

实现 [019 AutoSave 策略](019-autosave-design.md)：Rust 权威管理持久化开关和每文档 AutoSave 状态；Qt 只提供一次性 deadline 唤醒、另存为对话框和 worker 文件 I/O；QML 呈现开关及结果。不得改写 019 已定的用户路径、冲突和关闭语义。

## 必读

- [AutoSave 策略](019-autosave-design.md)、[运行时](../architecture/core/runtime.md)、[界面外壳](../architecture/interface/ui-shell.md)
- [文档生命周期与历史](018-document-lifecycle-history.md)、[Preferences 与常量](029-preferences-and-constants.md)
- [Rust crate 评估](../research/009-autosave-rust-library-audit.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)、[测试](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 在 Rust Settings/Preferences 持久化全局 AutoSave 偏好，默认关闭；为每份文档会话维护独立运行状态和触发 generation。
- 接入标题栏开关、未命名文档路径选择、debounce deadline、保存队列、外部修改检查、错误提示、冲突恢复和关闭流程。
- 复用现有 `.cdoc` 校验、原子写入和 Qt worker 保存边界。
- 不引入 Tokio 或专用 AutoSave crate；不做自动导入/导出、云同步、协作、永久版本历史或文档内 AutoSave 开关。

## 前置条件与待决策

019 的设计已独立评审并标记 done 后开始。本文中的触发间隔、恢复策略和取消语义均以 019 为准；如实现发现平台接口无法满足，先修订 019，再改实现。

## 实施步骤

1. 将 AutoSave 偏好纳入 Rust Settings/Preferences 序列化、校验、恢复和 ViewModel 投影；默认值保持关闭。
2. 在 Rust 文档应用服务实现每文档状态和基于可注入单调时钟的 deadline 决策。编辑只产生事件和新 deadline；Qt 设置单次 timer 唤醒后将 generation 回传，Rust 校验过期/重复事件。
3. 开启开关时对已有路径文档直接启用；未命名文档先请求另存为，取消时回滚偏好和 UI。应用在开启偏好下启动为未命名文档时同样请求路径；已有 `.cdoc` 打开不弹窗。
4. 让手动保存与 AutoSave 共享每文档按顺序执行的写入队列；保存回调校验会话和 revision，旧结果不得覆盖新状态。
5. 新增 `app-local-data:` 路径根映射到 Qt `QStandardPaths::AppLocalDataLocation`，并在 `caret-app` 内拆出独立 `DocumentRecoveryStorage` 服务；它管理单文件 envelope 原子读写、恢复和 30 天清理，不把这些策略塞进 `DocumentStorage`。保存前校验外部文件基线；AutoSave 冲突或目标失效时不覆盖，并保存本地冲突快照。接入启动恢复提示；恢复到临时会话后继续保留并随编辑更新该 envelope，直到另存成功或明确丢弃。
6. 手动保存遇到外部冲突时提供“另存副本 / 确认替换外部版本 / Cancel”；确认替换前保存外部原始字节到 recovery envelope，备份失败则禁止替换。接入 AutoSave 错误/冲突提示和关闭前 flush；失败、冲突、新 revision 未保存时沿用 018 的 Save / Don't Save / Cancel 对话框。
7. 按每个功能边界拆分 Rust、FFI、C++、QML 文件；增加命令/服务级测试、Qt 接线测试和实际桌面窗口验收，再更新架构与索引。

## 预计改动

`crates/caret-app/src/settings/`、`crates/caret-app/src/` 文档服务、独立 recovery storage 与路径根、`crates/caret-ffi/`、`native/viewmodels/document/`、标题栏及恢复/冲突 QML 组件；最终文件路径按现有模块边界确定。修改 [runtime](../architecture/core/runtime.md)、[界面外壳](../architecture/interface/ui-shell.md) 和本 task 的验证记录。

## 验收标准

- [ ] 默认关闭；开关值由 Rust 持久化，重启恢复正确；同偏好跨窗口一致、每文档运行状态不串线。
- [ ] 已命名 `.cdoc` 不触发另存为；从关闭切开启时，未命名文档立即请求路径，成功后再持久化开启，取消不改原关闭偏好。启动时偏好已开启但未命名文档取消路径选择时，尽力持久化关闭；若设置只读/写入失败，运行时仍暂停 AutoSave、显示设置错误，不能假称持久化成功。
- [ ] 多窗口路径选择串行化，使用独立 operation generation 加每文档 session generation；参与文档在操作中被切换开关、打开/新建/关闭时，旧流程失效，迟到回调不能重新开启全局偏好；已成功另存结果保留。
- [ ] 1 秒静默期和 30 秒最大等待由 Rust 决定；连续输入不会无限推迟保存；timer 重复、过期、关闭后迟到的 deadline 事件无副作用；无待处理工作时没有周期轮询。
- [ ] 连续编辑只安排必要的最新 revision；写入期间新修改在旧写入完成后再保存；旧 revision、旧 session 或旧 generation 回调不得错误推进当前保存点、标题状态或关闭窗口。
- [ ] 手动保存与自动保存同路径按请求顺序执行；同一目标路径不会让旧 revision 最终覆盖新 revision。
- [ ] 外部修改、删除、移动、不可读和摘要不匹配时自动写入拒绝覆盖；本地内容留在会话中，冲突恢复 envelope 写入失败也不丢内存内容。手动 Save 遇到冲突时只按明确选择另存或确认替换；替换前外部原始字节备份成功是强制条件。
- [ ] `.cdoc` 原子替换失败不破坏旧文件；恢复 envelope 自身原子写入；恢复取消/失败不删除条目。恢复到临时会话后再次崩溃仍可从原 envelope 恢复；只有另存成功或显式丢弃后才删除。
- [ ] 恢复项启动可见、30 天后过期清理只触及 Caret 管理文件；被占用/无权限时不阻塞正常启动。
- [ ] 关闭时 AutoSave 先 flush 当前 revision；失败、冲突或较新编辑存在时展示既有三选项；Cancel 保留会话，Don't Save 清除其恢复项。
- [ ] 错误状态可通过界面解释；文档标题状态仍严格遵守 018 的 `Edited`、`Saving`、`Saved`。
- [ ] 覆盖注入时钟下的 deadline、设置 ReadOnly/提交失败、多窗口 operation generation、开关取消、并发保存、冲突/显式替换备份、恢复再次崩溃、清理、关闭、跨层通知及窗口行为；Windows/macOS/Linux CI 与本地可用的窗口验收通过。
- [ ] 独立 subagent 最终审查完整 diff，清理未使用依赖、孤儿文件和重复实现；task-index 和架构文档与实现一致。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 偏好、deadline、队列和恢复测试 | 测试 019 规定的成功、失败、取消、过期事件和竞争路径 | 未执行，任务尚未开始。 |
| — | Qt/QML 组件和窗口测试 | 开关、路径选择、冲突提示、关闭及恢复流程正确 | 未执行，任务尚未开始。 |
| — | Windows/macOS/Linux CI | 三平台编译、lint、测试与资源注册通过 | 未执行，任务尚未开始。 |

## 风险与回退

AutoSave 触及用户文件且可能与外部编辑器竞争。任何基线校验或原子写入不确定性都不得通过直接截断文件规避；冲突时拒绝覆盖并保留本地快照。若平台无法可靠处理冲突 envelope，首版可将 AutoSave 限制在支持的本地路径并明确报错，不降低既有手动保存保证。

## 决策与工作记录

- 2026-10-28：依据 019 创建实现任务；实现尚未开始。

## 完成摘要

未完成。
