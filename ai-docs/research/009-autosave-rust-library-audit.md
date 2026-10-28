# Rust AutoSave 库评估

- 查阅日期：2026-10-28
- 目的：为 Caret 的 AutoSave 策略选择可复用 Rust 库，避免重复实现已有通用能力。

## Caret 需求

Caret 的 AutoSave 不是简单地定时序列化一个值。它需要和 Rust 文档会话的 revision/savepoint、`PreferencesService`、另存为路径绑定、Qt worker 写入、每文档串行保存、外部修改冲突检测以及关闭状态机协作。仓库没有 Tokio runtime；桌面 I/O 目前由 Qt worker 执行，Rust 应用服务保有策略和权威状态。已有 `DocumentStorage::save_snapshot` 校验 `.cdoc` 并复用同目录临时文件、同步和原子替换实现。

## 候选评估

| 候选 | 能力 | 与 Caret 的适配 |
| --- | --- | --- |
| [`autosave`](https://docs.rs/autosave/latest/autosave/) | 文档列出 `client`、`config`、`daemon`、`git`、`types`、`watcher` 模块，概览仅说明是 AutoSave 库。 | 没有呈现可复用的编辑器 revision、路径绑定、并发保存或冲突恢复 API；不能替代 Caret 应用服务。 |
| [`debounced`](https://docs.rs/debounced/latest/debounced/) | 对 Future 或 Stream 做尾沿延迟/防抖，当前只提供 trailing debounce。 | 是通用异步流工具，不能管理文档保存点、会话代际或文件冲突；Caret 当前以 Qt worker 和事件循环为异步边界，接入该库需要额外 Future/Stream 管线。 |
| [`tokio-debouncer`](https://docs.rs/tokio-debouncer/) | Tokio task 的可取消 leading/trailing debouncer，支持模拟时间测试。 | 依赖 Tokio task/runtime；仓库没有 Tokio runtime，为一个 deadline 引入整套异步运行时不合适。 |
| [`notify`](https://docs.rs/notify/latest/notify/) | 跨平台文件系统通知，可提供外部文件变化的提前信号。 | 可作为未来冲突提示优化，但不能作为正确性依据：文档列出网络文件系统可能不发事件，某些平台/挂载也有通知限制；写前仍需要基线验证。AutoSave 首版不需要常驻 watcher。 |
| [`fs4`](https://docs.rs/fs4/latest/fs4/) | 跨平台文件工具及 advisory lock。 | Workspace 已有该依赖，但当前仅供构建工具使用。协作锁只对配合的进程有效，不能保护来自普通编辑器或不使用锁的应用的修改，因此不能替代内容基线检查；首版不新增锁依赖到 `caret-app`。 |

## 决策

不新增专用 AutoSave crate，也不引入 Tokio。Rust `caret-app` 新增的 AutoSave 服务负责策略、单调 deadline、generation、每文档状态和冲突规则；Qt 事件循环设置单次唤醒并沿用已有 worker 写盘路径；`DocumentStorage` 继续负责 `.cdoc` 校验和原子写入。尾沿与最大等待只需要小型确定性状态，不值得为了防抖依赖另建异步运行时。

内容摘要用于侦测外部文件变化。若实现决定使用摘要，应复用 workspace 已有的 `sha2` 版本，并按 Cargo 依赖规范显式加入 `caret-app`；不要复制哈希算法实现。单纯用文件时间/大小可用于快速跳过，但不能作为唯一正确性检查。即使摘要检查通过，通用文件系统也没有便携的 compare-and-swap 替换操作，仍须如 AutoSave 设计所述保留冲突副本并说明竞态限制。

恢复副本应使用 Qt `QStandardPaths::AppLocalDataLocation`，通过独立的 `app-local-data:` 逻辑路径根注入 Rust。Qt 文档说明该位置在 Windows 是本机 AppData 目录，在 macOS 与其他平台等同 `AppDataLocation`；适合需要跨重启保留、但不应随用户漫游配置同步的大型恢复文件。具体目录名由组织名、应用名和打包环境决定，因此运行时以 Qt 的 `writableLocation()` 返回值为准，不在 Rust 拼接系统路径。参见 [Qt QStandardPaths](https://doc.qt.io/qt-6/qstandardpaths.html)，查阅于 2026-10-28。

## 来源

- [`autosave` crate 文档](https://docs.rs/autosave/latest/autosave/)，查阅于 2026-10-28。
- [`debounced` crate 文档](https://docs.rs/debounced/latest/debounced/)，查阅于 2026-10-28。
- [`tokio-debouncer` crate 文档](https://docs.rs/tokio-debouncer/)，查阅于 2026-10-28。
- [`notify` crate 文档和已知问题](https://docs.rs/notify/latest/notify/)，查阅于 2026-10-28。
- [`fs4` crate 文档](https://docs.rs/fs4/latest/fs4/)，查阅于 2026-10-28。
- [Qt `QStandardPaths` 文档](https://doc.qt.io/qt-6/qstandardpaths.html)，查阅于 2026-10-28。

## 结论

AutoSave 属于 Caret 的文档领域策略，库只在通用的文件通知、摘要或锁能力确实需要时单独评估。按当前架构，不值得用现成 crate 替代状态机，也不应为了 debounce 引入 Tokio。
