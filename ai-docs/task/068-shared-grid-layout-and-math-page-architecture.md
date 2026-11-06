# 068 — 共享网格几何与数学页面适配架构优化

- 状态：done
- 依赖：042、043
- 优先级：P2
- 创建 / 更新：2026-11-06 / 2026-11-06

## 目标与背景

梳理多行数学、文档表格与普通列表的共同几何职责，将可复用的已测量网格定位算法放到不依赖数学、文档模型或 UI 的低层 crate；各领域适配器保留自己的列宽策略、文字重排、编号、基线语义、分页、裁剪与命中契约。明确字体角色接口、数学页面快照及跨 crate 依赖的稳定归属。

本任务记录架构抽取、边界与验证，不替代 044 的公式画布/交互验收，也不替代 046 表格布局或 051 列表接入。当前 `caret-grid-layout` 由数学适配器实际消费；文档表格和列表是否以及如何复用，须在各自任务实现时验证。

## 必读

- [Rust 分层规范](../standards/layering/rust.md)、[依赖图](../standards/layering/dependency-graphs.md)
- [源码署名](../standards/source-attribution.md)、[仓库文件结构](../standards/repository-hygiene.md)、[注释与接口契约](../standards/comments.md)
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)
- [公式与表格架构](../architecture/editor/math-and-table.md)、[表格](../architecture/editor/tables.md)、[字体选择](../architecture/editor/font-selection.md)
- [044 数学布局与画布](044-math-layout-and-rendering.md)、[046 表格布局与画布](046-table-layout-and-canvas.md)、[051 代码、引用、链接与列表布局](051-code-links-quotes-layout.md)

## 范围与非目标

- 抽取并验证不含领域策略的网格几何：测量单元格、行列位置、基线、对齐和网格线。
- 保持数学后端仅负责 MathFlow/MATH 度量适配；通用页面快照与绘制边界不复制数学语义。
- 建立 L1 `caret-font-roles` 共享接口，集中管理字体角色、默认族名和所有静态字体字节；移除 L3 同级依赖，并让 `FontSelectionContext` 使用单一语义角色。偏好匹配与候选策略继续由 `caret-layout` 持有，MATH 表度量仍由 `caret-math` 管理。
- 记录 044 当前接入的真实状态和剩余项；表格与列表的生产接入分别留给 046、051。

不在本任务中完成公式编辑命中/光标、命令可用性与 `.cdoc` 可写门控，不实现完整文档表格列宽算法、列表编号/嵌套/跨页策略，也不以抽取 crate 宣称性能提升。

## 前置条件与待决策

042、043 已完成。用户确认字体策略和代码环境会扩展，要求现在建立清晰的共享边界，不接受 L3 同级调用，并指定所有静态字体资源由 L1 `caret-font-roles` 管理。该 crate 定义 Body、Math、Code 和默认族名并提供 JetBrains Mono faces / STIX bytes；偏好、脚本/语言规则、候选顺序和 Fontique 查询留在 `caret-layout`，MATH 表度量留在 `caret-math`。未来代码领域 crate 在 050/051 有真实独立语义 API 时按 L3 创建，不依赖布局/数学同级 crate；纯资源 crate 已由 `caret-font-roles` 取代，不保留空壳。此整理不表示文档 Math 字体偏好已接入公式布局。

## 实施步骤

1. 对照依赖图确认允许方向，并记录共享算法输入、输出、失败契约和调用方仍持有的策略。
2. 抽取网格几何并接入现有数学适配器，保留 Typst 来源、许可证及具体修改记录。
3. 修复跨层输入映射、公式图元与页面坐标契约，补正常、边界和失败行为测试。
4. 建立共享字体角色契约并移除 L3 同级边；同步 044/046/051 的已实现、计划与未覆盖状态；做 Terra 分领域检查和独立 Sol 完整 diff 评审，处理发现并复验。

## 预计改动

`crates/caret-grid-layout/`、`crates/caret-font-roles/`、`caret-math` 数学适配、`caret-layout` 行内对象/字体角色/分页、`caret-engine` 页面投影与字形快照、必要的 FFI/native 绘制适配、分层和任务文档。

## 验收标准

- [x] `caret-grid-layout` 无 workspace 依赖，公共几何 API 的单位、坐标、signed 度量、溢出与失败契约完整；整行扩宽时右边界线跟随最终宽度。
- [x] 数学适配器复用共享几何且保留 Typst 来源；有效负空白和带符号字形度量不丢内容。
- [x] 行内公式占位映射不重复塑形或 advance；公式周围文本的源位置、affinity、RTL 逻辑端点和部分 UTF-8 选区映射均有回归。
- [x] 页面图元及公式片段坐标跨 Rust/FFI/native 契约一致；默认前景描边保留；QSG 绘制顺序、填色文字和裁剪边界有像素级回归。
- [x] L3 crate 之间没有生产依赖边；字体角色、默认名和 Rust 静态字节接口归无 workspace 依赖的 `caret-font-roles`；Qt 与 Rust 读取同一资源文件，用户偏好策略仍由 `caret-layout` 持有；引擎绘制字节与布局所用字体一致。
- [x] 046/051 清楚说明复用边界，且不把尚未存在的接入写成已完成。
- [x] 本任务实际命令、环境、结果、分领域审查、Sol final review 和未覆盖项均已记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-06 | `cargo test --workspace --locked --offline` | Rust workspace 与直接消费者回归通过 | 通过；包含 `caret-grid-layout` 10 项、`caret-layout` 111 项、`caret-engine` 541 项，以及 native CTest 9/9 |
| 2026-11-06 | 受影响 crate 的 `cargo clippy --locked --offline --all-targets -- -D warnings` | 公共接口及测试无 lint 错误 | 通过；覆盖 font-roles、grid-layout、doc、layout、math、engine、app、ffi |
| 2026-11-06 | `cargo format --check`、`cargo fmt --all -- --check`、`cargo lint --check` | 跨语言格式、依赖分层及本地 lint 检查通过 | 通过；`cargo lint` 九步均通过，含 clang-tidy、include-cleaner、cppcheck、QML 与 native 构建 |
| 2026-11-06 | `cargo metadata --locked --offline --no-deps --format-version 1`；layout/math `cargo tree --locked --offline -e normal --depth 1` | workspace 元数据有效；无 L3 peer edge，字体角色和共享网格依赖方向正确 | 通过；layout 只直接依赖 doc/font-roles，math 依赖 doc/font-roles/grid-layout |
| 2026-11-06 | `cargo quality` | 完整 lint、依赖审计和 workspace 测试完成 | lint 九步通过后，cargo-deny advisory DB 无法取得 `/Users/yuki/.cargo/advisory-dbs/db.lock` 的写锁（该路径在沙箱中只读）；workspace 测试已另行通过，advisory 检查仍未验证 |
| 2026-11-06 | Terra/medium 架构与渲染分领域 review；Sol/medium 完整 diff review | 领域发现修复，独立 final review 无未解决项 | 通过；Terra 发现均已修复并复验，Sol 对完整 diff 未发现剩余问题；044 文档修订经 Sol 复核 |

## 风险与回退

Parley 对 InlineBox 与文本索引的语义、signed MATH 墨迹度量和页面图元传输容易造成静默视觉/命中偏差。若一个领域的共享 API 不足以表达其策略，应将策略留在适配器，不以扩张通用几何 API 掩盖差异。性能收益无基线前标为未验证。

## 决策与工作记录

- 2026-11-06：从 044 分支的架构整理中单独登记共享网格与数学页面边界，避免把架构抽取误记为 044/046/051 的功能验收。
- 2026-11-06：按维护者方向在本任务内建立 L1 字体角色/资源 crate，删除 `caret-layout → caret-math`，并将 `FontSelectionContext` 的双布尔标志收敛为单一 `FontRole`；不保留无消费者的字体族名转发。
- 2026-11-06：Terra/medium 审计确认 `caret-layout → caret-code` 原本是合法向下边，但用户明确未来代码环境是与 Math 并列的领域能力。因 `caret-code` 当时仅承载字体资源，本任务改为让 `caret-font-roles` 统一管理所有静态字节并删除该空壳；050/051 记录未来真实代码领域 crate 与 `caret-math` 同属 L3、零同级依赖。
- 2026-11-06：`caret-engine` 页面绘制改从 `caret-font-roles` 取 STIX 字节，与 `MathFont::builtin()` 共用同一资源；自定义 MathFont 的页面绘制仍未支持，按 044 后续验收记录。
- 2026-11-06：Terra review 发现并推动修复：paint-order/描边/clip、inline affinity/RTL/部分字节范围、网格扩宽后的右边界线位置，以及过时任务说明和注释。Qt 像素回归使用变换后的矩形笔画，避免依赖平台受限的宽线 primitive。
- 2026-11-06：`cargo quality` 的 advisory 子检查因沙箱无法写入宿主 Cargo advisory DB 锁文件未完成；`cargo test --workspace --locked --offline` 和 `cargo lint --check` 分别通过。三平台 Runtime 打包 workflow 未执行。

## 完成摘要

共享网格几何、字体角色边界、数学页图元传输/绘制及相关交互映射审查整改已完成并通过 workspace 验证。`cargo quality` 的 advisory DB 检查受沙箱只读限制；三平台 Runtime 打包 smoke 未执行。044 的公式内部命中/光标、命令和持久化验收仍保持 in-progress。
