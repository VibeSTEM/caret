# 101 — native 边界协议与渲染期重复校验收敛

- 状态：done
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

Brooks sweep 在 native 层已完成 canvas 内部 glyph frame 线格式单解析（`glyph_frame_wire`）与打开/恢复异步流程共享，但仍有跨层/渲染期重复：
- `native/viewmodels/document/layout_snapshot_codec.cpp` 把 Rust 侧协议决策当字符串词表逐条复制（command_line mode 7 个字符串、`composition` 状态推导等），且自行校验 Rust 布局结果的“语义正确性”（sourceStart/consumed 铺满段落、自造浮点容差），属信息泄漏与跨语言双份权威；
- `native/canvas/document_canvas/scene_graph.cpp` 在每次 scene 重建时仍整体 `parse` glyph frame（实现去重了，工作量未去重），并在缺纹理检查中残留一份 image 线格式判定（与 wire 规则当前一致但可漂移）；
- 页面局部矩形（`pageIndex/x0Pt/y0Pt/x1Pt/y1Pt`）的校验在 codec、environment、selection 等处手写 4 份。

完成后：命令行属性键、外层 `commandLine` 键、模式名、缺省前缀与校验策略在共享描述中定义；Rust 生成运行时映射，standalone CMake 生成 native 读取头。native codec 读取的 `compositionPreview` 键也由该描述提供；布局增量的 `partial` 和其他快照字段继续沿用 Rust 的既有 Serde camelCase 契约。渲染期不重复解析未变化帧，矩形几何校验按各自信任边界复用规则且保留入口策略。打开与恢复的安装失败信号使用相同的错误前缀剥离规则。

## 必读

[分层：native](../standards/layering/native.md)（canvas 与 ViewModel 互不链接）、[FFI](../standards/ffi.md)、[架构：画布](../architecture/interface/canvas-highlighting.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：命令行协议词表单源（Rust 导出或共享生成）、scene_graph 复用已解析结果/收敛 image 判定、保留入口策略的矩形校验规则复用，以及打开/恢复安装失败时统一剥离内部 `error:` 前缀。非目标：改线格式（JSON 字段名/类型）、错误语义和文档状态行为；不合并跨信任边界的 adapter/canvas 校验（分层要求保留）。

## 前置条件与待决策

协议采用共享 JSON 描述，避免 APP=OFF 的 native 模块构建重复维护 Rust 词表；Rust build script 和 CMake 分别生成各自入口。页面节点缓存已解析帧，帧身份变化时重新解析；节点删除时随页面生命周期释放缓存。

## 实施步骤

1. 列出 `layout_snapshot_codec.cpp` 复制的协议词表与 Rust 来源，逐项给出单源方案（导出/共享生成/映射）。
2. 先做无行为变化的单源迁移；再收敛 scene_graph 的 image 判定与重复 parse（缓存到 PageGlyphNode，失效条件与刷新路径明确）。
3. 矩形校验逐入口列出输入类型、信任边界和几何策略；抽取不改变策略的纯校验规则，但保留 ViewModel/Canvas 各自的适配和边界检查。拖选预览允许超出页面的矩形时，不得复用会拒绝越界的严格规则。
4. 打开与恢复安装失败统一剥离 Rust 响应的 `error:` 前缀，同时保留恢复失败后的恢复列表刷新；非空响应仍一律判为失败。补恢复条目准备成功、安装前会话过期的 Qt Test，断言错误文本无前缀、当前文档内容保留且恢复列表刷新。
5. native 验证：`cargo test --locked -p caret-tests --test native`、clang-format/tidy、增量构建 0 新增告警；全量门禁 + 独立评审。

## 预计改动

`shared/command_line_protocol.json`、`crates/caret-engine/build.rs`、Rust/CXX protocol bridge、`native/viewmodels/document/layout_snapshot_codec.cpp`、`native/viewmodels/CMakeLists.txt`、`native/canvas/document_canvas/scene_graph.cpp`、共享页面矩形校验头及相关 Qt/Rust 测试；不改 Rust 线格式。

## 验收标准

- [x] 协议词表不再手写双份，漂移有共享 JSON 描述与生成保证。
- [x] scene_graph 不再在每次重建时整体重复 parse；image 线格式判定只有一处。
- [x] 矩形规则在信任边界内复用且入口策略一致；拖选预览越界与普通选区拒绝越界的既有差异由测试固定。
- [x] 打开/恢复的安装错误都剥离 `error:`，恢复失败会刷新列表；过期会话恢复测试证明错误文本、文档保留与刷新行为。
- [x] native 与全量 workspace 测试通过；格式、lint、依赖审计及独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo test --locked -p caret-tests --test native` | native CTest 全部通过 | 20/20 通过；含新增的选区边界入口行为、布局快照和 QML shell 测试 |
| 2026-11-18 | `cargo test --locked --workspace` | 全部 Rust 与 native 测试通过 | 通过；native CTest 20/20，Rust crate 与 integration tests 全部通过 |
| 2026-11-18 | `cargo format --check`、`cargo lint --check` | 格式与完整 lint 通过 | 通过；含 Clippy、CMake 分层、qmllint、clang-tidy、include-cleaner 和 cppcheck |
| 2026-11-18 | standalone CMake：`CARET_ENABLE_APP=OFF`，构建 `caret_viewmodels` | 无 FFI 的 native 模块构建通过 | 通过；共享协议 JSON 在 standalone 配置中正常生成并编译 |
| 2026-11-18 | `CARGO_HOME=/private/tmp/caret-cargo-home cargo deny --offline check` | advisory、bans、licenses、sources 检查通过 | 通过；advisories/bans/licenses/sources 均 ok，仅有 5 条既有未匹配许可证白名单警告 |
| 2026-11-18 | `cargo quality` | 聚合质量门禁通过 | 各 lint 项已通过；aggregate 的 cargo-deny 在线刷新因沙箱 DNS 不可用失败，使用缓存数据库的离线 cargo-deny 检查另行通过 |

## 风险与回退

渲染缓存仅在页面节点内存活，frame identity 变化时重新解析，页面离开可见集合时释放。共享协议描述的 Rust 与 CMake 生成路径都由同一 JSON 输入触发重建。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已完成 glyph frame canvas 内部单解析与异步流程共享；跨层协议与渲染期重复留本任务。
- 2026-11-17：Issue #140 统一打开/恢复安装失败的 `error:` 前缀剥离；新增过期会话恢复行为断言，验证错误文本、当前文档保留和恢复列表刷新。其余协议单源、渲染缓存与矩形策略项仍待实施。
- 2026-11-18：协议决策采用 `shared/command_line_protocol.json`；Rust build script 从中生成命令行字段名、外层 `commandLine` 键、模式名、反查、缺省前缀和验证器，CMake 将同一描述嵌入 standalone codec，保留 APP=OFF 的协议行为。codec 消费的 `compositionPreview` 键取自字段映射；`partial` 与其他布局增量字段继续使用 Rust 既有 Serde camelCase 契约，不纳入共享字段表。
- 2026-11-18：`PageGlyphNode` 缓存已解析 `GlyphFrame`，仅当 frame identity 变化时解析；缺纹理重试读取已解析的图像尺寸，不再复制像素长度校验。
- 2026-11-18：抽取页面局部矩形的坐标有效性与页面包含性。JSON/ViewModel 和 Canvas 继续独立做输入类型、索引及边界策略适配；Qt Test 固定普通选区拒绝越界且保留旧值、拖选预览接受同一越界矩形。
- 2026-11-18：移除 C++ 对 Rust 行布局完整覆盖和行间 top 连续性的重复推断，保留范围边界、UTF-8 边界、有限值和 Qt 消费安全检查；测试分别覆盖合法 producer 差异和非法 UTF-8 拒绝。
- 2026-11-18：Terra 领域评审与未参与实现的 Sol 最终完整 diff 评审通过；最终复核确认完整/轻量快照和命令回复分流均通过共享键名读取 `commandLine`，`partial` 保持既有 Serde 契约。

## 完成摘要

完成 task-101：协议定义单源化，场景图缓存已解析帧，矩形规则复用并保留入口策略，布局 codec 不再重复裁定 Rust 产出语义。打开/恢复统一错误前缀行为沿用前置实现。所有任务验收项和独立评审完成。
