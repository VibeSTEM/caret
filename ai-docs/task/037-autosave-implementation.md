# 037 — AutoSave 实现

- 状态：in-progress
- 依赖：018、019、029
- 优先级：P2
- 创建 / 更新：2026-10-28 / 2026-10-29

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
5. 新增 `app-local-data:` 路径根映射到 Qt `QStandardPaths::AppLocalDataLocation`，并在 `caret-app` 内拆出独立 `DocumentRecoveryStorage` 服务；它管理单文件 envelope 原子读写、恢复和 30 天无更新清理。跨进程共享租约阻止其他运行实例清理或操作仍由会话持有的恢复项。不要把恢复策略塞进 `DocumentStorage`。保存前校验外部文件基线；AutoSave 冲突或目标失效时不覆盖，并在创建冲突快照时立即持有租约，绑定到文档会话后持续更新。恢复时锁定原条目，验证并准备成功后创建会话专属副本；只有候选会话成功安装后才移除来源，失败或取消保留来源。
6. 手动保存遇到外部冲突时提供“另存副本 / 确认替换外部版本 / Cancel”；确认替换前保存外部原始字节到 recovery envelope，备份失败则禁止替换。接入 AutoSave 错误/冲突提示和关闭前 flush；失败、冲突、新 revision 未保存时沿用 018 的 Save / Don't Save / Cancel 对话框。
7. 按每个功能边界拆分 Rust、FFI、C++、QML 文件；增加命令/服务级测试、Qt 接线测试和实际桌面窗口验收，再更新架构与索引。

## 预计改动

`crates/caret-app/src/settings/`、`crates/caret-app/src/` 文档服务、独立 recovery storage 与路径根、`crates/caret-ffi/`、`native/viewmodels/document/`、标题栏及恢复/冲突 QML 组件；最终文件路径按现有模块边界确定。修改 [runtime](../architecture/core/runtime.md)、[界面外壳](../architecture/interface/ui-shell.md) 和本 task 的验证记录。

## 验收标准

- [ ] 默认关闭；开关值由 Rust 持久化，重启恢复正确；当前单窗口内每文档运行状态不串线。跨窗口共享偏好与运行时协调转由 [038](038-multiwindow-document-sessions.md) 验收。
- [ ] 已命名 `.cdoc` 不触发另存为；从关闭切开启时，未命名文档立即请求路径，成功后再持久化开启，取消不改原关闭偏好。启动时偏好已开启但未命名文档取消路径选择时，尽力持久化关闭；若设置只读/写入失败，运行时仍暂停 AutoSave、显示设置错误，不能假称持久化成功。
- [ ] 当前窗口的路径选择流程按 session generation 失效；迟到回调不能更改被替换文档的状态。跨窗口路径选择串行化、独立 operation generation 和文档集合变更失效由 038 验收。
- [ ] 1 秒静默期和 30 秒最大等待由 Rust 决定；连续输入不会无限推迟保存；timer 重复、过期、关闭后迟到的 deadline 事件无副作用；无待处理工作时没有周期轮询。
- [ ] 连续编辑只安排必要的最新 revision；写入期间新修改在旧写入完成后再保存；旧 revision、旧 session 或旧 generation 回调不得错误推进当前保存点、标题状态或关闭窗口。
- [ ] 手动保存与自动保存同路径按请求顺序执行；同一目标路径不会让旧 revision 最终覆盖新 revision。
- [ ] 外部修改、删除、移动、不可读和摘要不匹配时自动写入拒绝覆盖；本地内容留在会话中，冲突恢复 envelope 写入失败也不丢内存内容。手动 Save 遇到冲突时只按明确选择另存或确认替换；替换前外部原始字节备份成功是强制条件。
- [ ] `.cdoc` 原子替换失败不破坏旧文件；恢复 envelope 自身原子写入；恢复取消/失败不删除条目。恢复到临时会话后再次崩溃仍可从原 envelope 恢复；只有另存成功或显式丢弃后才删除。
- [ ] 恢复项启动可见、30 天无更新后清理只触及 Caret 管理文件；被其他应用实例持有时不清理，被占用/无权限时不阻塞正常启动。
- [ ] 关闭时 AutoSave 先 flush 当前 revision；失败、冲突或较新编辑存在时展示既有三选项；Cancel 保留会话，Don't Save 清除其恢复项。
- [ ] 错误状态可通过界面解释；文档标题状态仍严格遵守 018 的 `Edited`、`Saving`、`Saved`。
- [ ] 覆盖注入时钟下的 deadline、设置 ReadOnly/提交失败、开关取消、并发保存、冲突/显式替换备份、恢复再次崩溃、条目并发、清理、关闭、跨层通知及窗口行为；Windows/macOS/Linux CI 与本地可用的窗口验收通过。
- [ ] 独立 subagent 最终审查完整 diff，清理未使用依赖、孤儿文件和重复实现；task-index 和架构文档与实现一致。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-28 | `cargo test --offline --workspace --locked` | 设置、deadline、recovery envelope、文件冲突、编辑会话及 workspace 集成 | 通过；caret-app 119 项、caret-ffi 10 项，其他 workspace 和集成测试全部通过；native CTest 4/4。 |
| 2026-10-28 | `cargo clippy --offline --workspace --all-targets --locked -- -D warnings`、`cargo fmt --all --check`、`cargo format --check` | Rust lint 与跨语言格式检查 | 通过。 |
| 2026-10-28 | `cargo quality`、`CARGO_HOME=/private/tmp/caret-cargo-home cargo deny check` | 完整静态质量与依赖安全检查 | `cargo quality` 的前 8/9 项通过；第 9 项默认 sandbox Cargo advisory 锁不可写。使用可写隔离 Cargo home 并获准更新 RustSec 数据后单独执行 cargo-deny，advisories/bans/licenses/sources 均通过；仅有既有未命中 license allowance 警告。 |
| 2026-10-28 | `cargo test --offline -p caret-ffi --lib`、`cargo build --offline -p caret-ffi` | FFI 快照、设置、deadline 与保存点接线 | 通过：10 项 FFI 单测及 debug 静态库构建。 |
| 2026-10-28 | `cargo test --offline --workspace --locked`、`cargo clippy --offline --workspace --all-targets --locked -- -D warnings` | 当前 Rust workspace 全测试与 lint | 通过；Rust 集成测试触发本机构建与 CTest 4/4。 |
| 2026-10-28 | `cmake --build /tmp/caret-autosave-app-build --parallel 4`、`ctest --test-dir /tmp/caret-autosave-app-build --output-on-failure` | Qt/QML 构建、自动保存写入、冲突目标回归与格式检查 | 通过；当前变更后 CTest 3/3。新增“文档 A 冲突后另存 B，再遇 AutoSave 冲突不得 Replace A”的 Qt 回归测试。workspace native CTest 另通过 4/4。 |
| 2026-10-28 | macOS 桌面手工验收 | 未命名文档开启 AutoSave 先请求路径；输入后状态按 Edited → Saved 更新 | 通过：另存至 `/private/tmp/caret-autosave-manual.cdoc` 后输入文本，标题先显示 Edited，静默期后显示 Saved；AutoSave 偏好恢复为原关闭状态，临时文档已删除。未人工操作恢复、冲突和关闭对话框。 |
| 2026-10-29 | macOS 当前桌面构建手工验收 | 未命名文档取消 AutoSave 路径选择后开关回到关闭；关闭已编辑文档时 Cancel 保留文档与 Edited 状态，Don't Save 关闭文档 | 通过。使用临时未命名文档测试，未写入用户文件；关闭后重新启动，显示干净空白文档且 AutoSave 仍为关闭。 |
| 2026-10-29 | `cargo test --offline --workspace --locked` | 完整 Rust workspace、CLI 集成与本机 native CTest | 通过；caret-app 127 项、caret-ffi 10 项、文档集成 19 项；native CTest 4/4。新增独立子进程持有恢复租约时的隔离测试，以及 AutoSave 重新启用时恢复当前脏 revision 的策略测试。 |
| 2026-10-29 | `cargo lint --check`、`cargo fmt --all --check`、`cargo format --check`、staged/worktree `git diff --check` | Rust、依赖、CMake、分层、QML、clang-tidy、include-cleaner、cppcheck 与格式/空白检查 | 全部通过；lint 的 9 个阶段均通过。 |
| 2026-10-29 | `cmake --build /tmp/caret-autosave-app-build --parallel 4`、`ctest --test-dir /tmp/caret-autosave-app-build --output-on-failure` | Qt/QML 桌面构建、Shell 加载和 QML 格式 | 通过；CTest 3/3。 |
| 2026-10-29 | Actions run [40725738442](https://github.com/VibeSTEM/caret/actions/runs/40725738442) | 三平台 Nightly 与 Miri、Rust 覆盖率通过 | macOS/Windows 及多数 Linux job 通过；Linux workspace/coverage 的恢复锁测试偶发 `recovery.entry_busy`，Miri 的跨进程测试触发不支持的 `posix_spawnattr_init`。本分支显式先解锁再清理进程内租约标记，并将跨进程测试标记为 Miri 忽略；本机 Miri 通过，恢复测试 40 轮并发重复通过。 |
| 2026-10-29 | 恢复锁并发稳定性复测 | 锁释放不受子进程短暂继承的文件描述符延长；恢复测试稳定 | 将恢复锁封装为显式 `unlock` 的 RAII guard，Session lease 在 active ID 锁内释放文件锁；新增重复句柄回归测试。本地完整恢复测试套件连续 300 轮通过（每轮 10 项测试、8 线程）；远端 Linux Nightly 仍待推送后复验。 |
| 2026-10-29 | Actions run [40732895150](https://github.com/VibeSTEM/caret/actions/runs/40732895150/job/122974451312) | Ubuntu clang-tidy/include-cleaner 通过 | 失败因 Qt Test 鼠标 API 缺少直接头文件 `qtestmouse.h`；已补 include，本地完整 `cargo lint --check` 通过。 |
| 2026-10-29 | 上述 Nightly / CI runs | 三平台编译、lint、测试与资源注册通过 | macOS 与 Windows workspace 通过；Linux/Miri 本次失败原因已在当前分支修复并本机复验，仍需推送后取得远程复验结果。 |

## 风险与回退

AutoSave 触及用户文件且可能与外部编辑器竞争。任何基线校验或原子写入不确定性都不得通过直接截断文件规避；冲突时拒绝覆盖并保留本地快照。若平台无法可靠处理冲突 envelope，首版可将 AutoSave 限制在支持的本地路径并明确报错，不降低既有手动保存保证。

## 决策与工作记录

- 2026-10-28：开始实现偏好持久化、单调 deadline、恢复 envelope、文件基线冲突检查和 Qt 单次唤醒。启动恢复选择、跨平台及完整关闭状态链仍待完成。
- 2026-10-28：实现启动恢复列表、恢复到临时 dirty 会话、编辑后更新同一 envelope、保存/丢弃时清理、外部冲突选择及 AutoSave 关闭 flush。Rust FFI 与 Qt 文件命令拆为 autosave/recovery 模块；同一路径排队保存携带前一份本应用快照摘要，避免把自己的写入误判为外部冲突。
- 2026-10-28：加入 Qt AutoSave deadline 回归测试；完整本地 Rust/Qt/QML 测试与 Rust Clippy 通过。当前应用通过单窗口 singleton runtime 工作，多窗口路径选择串行化尚未实现或验证；Windows/Linux CI 和人工桌面对话框验收仍待完成，因此保持 in-progress。
- 2026-10-28：恢复存储增加跨进程共享租约；启动清理尝试获取排他租约，另一个 Caret 实例仍运行时跳过清理。恢复 envelope 更新时间按最近一次更新刷新，过期按 30 天无更新判断。补充外部原始版本另存和恢复操作关闭续接；修复恢复列表删除后失效选择。Rust、Qt/QML 和格式检查本地通过。独立评审确认代码级 P1/P2 已处理，仍要求多窗口协调、跨平台 CI 与人工窗口验收才能完成任务。
- 2026-10-28：按范围决策将多窗口窗口/会话架构与跨窗口 AutoSave 开启事务拆分为 038；037 继续验收当前单窗口行为。复核发现的保存冲突目标 P1、活动恢复项 P1 和恢复失败提前删除来源 P2 已逐项修复：冲突 Replace 必须匹配当前路径；恢复区读改写跨进程串行，活动条目持有租约，来源只在候选会话成功安装后删除。新增相应 Rust 与 Qt 回归测试。
- 2026-10-28：最新本地验证通过：workspace Rust 测试、全 workspace Clippy、Rust/跨语言格式、Qt/QML 构建与测试；macOS 手工验证未命名文档 AutoSave 路径选择和 Edited → Saved 更新。三平台 CI 和冲突/恢复/关闭对话框的手工操作仍未覆盖。
- 2026-10-28：修复恢复副本生命周期竞态：活动恢复 ID 由会话租约锁定并从恢复列表隐藏，普通加载/删除会被拒绝；恢复来源在验证、取消和安装失败时保留，安装成功后才清理。冲突快照现在创建后立即持有租约，并通过 FFI 待绑定表移交给当前会话；Qt 绑定失败会显示在文件操作错误中。针对租约隔离与恢复准备失败补充 Rust 回归测试。独立复核已确认此前恢复来源 P1/P2 关闭；正在复核最新冲突快照租约改动。
- 2026-10-29：补齐冲突恢复错误详情与对话框动态高度；修正 AutoSave 在关闭后重新开启时同一脏 revision 未重新调度的问题，并覆盖关闭重开、失败后重开、写入中重开等路径。重复设置为开启不会重试失败写入，也不改变现有 deadline。切换开关后立即恢复其 Rust 偏好投影绑定，取消另存为或偏好写入失败时 UI 不会残留为开启。新增子进程级租约测试，验证一个进程持有恢复项时另一进程无法列出、读取或删除该项，释放后可重新读取；恢复测试夹具使用唯一序号避免并行目录碰撞。workspace 全量测试通过：caret-app 127 项、caret-ffi 10 项、文档集成 19 项、native CTest 4/4；9 阶段 `cargo lint --check`、`cargo format --check`、Rust/QML 格式及 diff 空白检查通过。三平台 CI、恢复/冲突/关闭对话框人工流程仍未验证。
- 2026-10-29：CI 暴露本地恢复文件锁只依赖 `File::drop` 释放时点不确定的问题：子进程 spawn/exec 窗口可能暂时继承文件句柄，使恢复条目仍被判定占用。所有恢复操作、进程、清理和探测锁现在通过 RAII guard 在关闭前显式解锁；完整恢复测试套件本地连续 300 轮通过。远程 Linux/Miri 重新运行仍是 037 完成前置条件。

## 完成摘要

核心单窗口 AutoSave、恢复与关闭链已实现并通过本机集成测试；双进程恢复租约隔离已有子进程回归测试。多窗口文档会话与全局操作协调由 038 承接；037 仍需通过三平台 CI，并人工验收恢复、冲突和关闭对话框后才能标记 done。
