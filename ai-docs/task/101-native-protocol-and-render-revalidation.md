# 101 — native 边界协议与渲染期重复校验收敛

- 状态：planned
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

Brooks sweep 在 native 层已完成 canvas 内部 glyph frame 线格式单解析（`glyph_frame_wire`）与打开/恢复异步流程共享，但仍有跨层/渲染期重复：
- `native/viewmodels/document/layout_snapshot_codec.cpp` 把 Rust 侧协议决策当字符串词表逐条复制（command_line mode 7 个字符串、`composition` 状态推导等），且自行校验 Rust 布局结果的“语义正确性”（sourceStart/consumed 铺满段落、自造浮点容差），属信息泄漏与跨语言双份权威；
- `native/canvas/document_canvas/scene_graph.cpp` 在每次 scene 重建时仍整体 `parse` glyph frame（实现去重了，工作量未去重），并在缺纹理检查中残留一份 image 线格式判定（与 wire 规则当前一致但可漂移）；
- 页面局部矩形（`pageIndex/x0Pt/y0Pt/x1Pt/y1Pt`）的校验在 codec、environment、selection 等处手写 4 份。

完成后：Rust 协议只在一处定义并由 C++ 消费（字段名/状态枚举来自共享描述或生成），渲染期不重复解析已校验帧，矩形校验按各自信任边界复用规则且保留入口策略。打开与恢复的安装失败信号使用相同的错误前缀剥离规则。

## 必读

[分层：native](../standards/layering/native.md)（canvas 与 ViewModel 互不链接）、[FFI](../standards/ffi.md)、[架构：画布](../architecture/interface/canvas-highlighting.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：协议词表单源（Rust 导出或共享生成）、scene_graph 复用已解析结果/收敛 image 判定、保留入口策略的矩形校验规则复用，以及打开/恢复安装失败时统一剥离内部 `error:` 前缀。非目标：改线格式（JSON 字段名/类型）、错误语义和文档状态行为；不合并跨信任边界的 adapter/canvas 校验（分层要求保留）。

## 前置条件与待决策

需要确认是否允许引入“共享协议描述→C++ 常量/校验”的生成步骤（涉及构建管线），或采用 Rust 侧导出枚举值、C++ 只做映射。渲染期缓存 parse 结果需评估帧内存增量与刷新时机。

## 实施步骤

1. 列出 `layout_snapshot_codec.cpp` 复制的协议词表与 Rust 来源，逐项给出单源方案（导出/共享生成/映射）。
2. 先做无行为变化的单源迁移；再收敛 scene_graph 的 image 判定与重复 parse（缓存到 PageGlyphNode，失效条件与刷新路径明确）。
3. 矩形校验逐入口列出输入类型、信任边界和几何策略；抽取不改变策略的纯校验规则，但保留 ViewModel/Canvas 各自的适配和边界检查。拖选预览允许超出页面的矩形时，不得复用会拒绝越界的严格规则。
4. 打开与恢复安装失败统一剥离 Rust 响应的 `error:` 前缀，同时保留恢复失败后的恢复列表刷新；非空响应仍一律判为失败。补恢复条目准备成功、安装前会话过期的 Qt Test，断言错误文本无前缀、当前文档内容保留且恢复列表刷新。
5. native 验证：`cargo test --locked -p caret-tests --test native`（11/11）、clang-format/tidy、增量构建 0 新增告警；全量门禁 + 独立评审。

## 预计改动

`native/viewmodels/document/layout_snapshot_codec.cpp`、`native/viewmodels/document/layout_view_model_file_commands.cpp`、`native/canvas/document_canvas/scene_graph.cpp`、`native/canvas/document_canvas/glyph_frame_wire.*`、`tests/cpp/shell/shell_document_recovery_test.cpp`、可能的共享常量头；不改 Rust 线格式。

## 验收标准

- [ ] 协议词表不再手写双份，漂移有测试或生成保证。
- [ ] scene_graph 不再在每次重建时整体重复 parse；image 线格式判定只有一处。
- [ ] 矩形规则在信任边界内复用且入口策略一致；拖选预览越界与普通选区拒绝越界的既有差异由测试固定。
- [x] 打开/恢复的安装错误都剥离 `error:`，恢复失败会刷新列表；过期会话恢复测试证明错误文本、文档保留与刷新行为。
- [ ] native 11/11 与全量门禁通过；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-17 | `cargo test --locked -p caret-tests --test native` | 11/11 | 11/11 通过；含 `Qml.ShellLoad` 过期会话恢复、QML 格式和其余 native CTest |
| 2026-11-17 | `cargo format --check` | 跨语言格式检查通过 | 首次检查发现本次两处 C++ 排版问题；运行 `cargo format` 后通过 |

## 风险与回退

渲染期缓存引入状态，需明确失效条件，否则出现旧帧/错页；保留可一键关闭的缓存开关或先只做单源迁移。协议单源改动涉及构建管线时需单独 task 记录。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已完成 glyph frame canvas 内部单解析与异步流程共享；跨层协议与渲染期重复留本任务。
- 2026-11-17：Issue #140 统一打开/恢复安装失败的 `error:` 前缀剥离；新增过期会话恢复行为断言，验证错误文本、当前文档保留和恢复列表刷新。其余协议单源、渲染缓存与矩形策略项仍待实施。

## 完成摘要

未完成。
