# 027 — 页面画布、命中与布局快照

- 状态：in-progress
- 依赖：022、024、026
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-26

## 目标与背景

以单一 LayoutResult 绘制连续多页，并使文字、页面、插入点、选区和命中测试共用同一映射。

## 必读

- [界面外壳](../architecture/interface/ui-shell.md)、[页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)
- [光标与选区职责](../architecture/interface/caret.md)
- [增量排版](../architecture/editor/incremental-layout.md)、[分层](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

实现滚动工作区、纸张绘制、布局快照、文本命中、选区/光标几何、当前环境背景高亮，以及 zoom。高亮由光标位置派生，作用于最内层 EnvironmentNode：块级环境高亮容器，行内环境高亮文字范围；它只属于视图叠加，不序列化。异步结果携带文档 generation，过期结果不能覆盖新版本。跨页选区仍是一条逻辑选区。

光标与选区状态由 Rust 提供，页面内几何由 LayoutResult 提供，Canvas 只绘制。纸张、边框、光标和选区颜色来自 `qml/Themes/` token 并由 QML 注入；闪烁、粗细和焦点状态等视觉细节以[独立设计](../architecture/interface/caret.md)为准。

不包含复杂尺规/页眉页脚、增量排版缓存和性能门槛；基准留给 013。页面几何使用前缀索引和 scene graph 几何，避免全篇纹理和二次方页面扫描。

## 前置条件与待决策

022 输入位置、024 glyph cluster、026 page layout 结构稳定；明确 C++ 画布、FFI 和 Rust 视图边界。

## 实施步骤

1. 定义 LayoutResult 到画布快照的最小 DTO。
2. 统一页面变换、命中、选区和绘制坐标。
3. 接入 generation 过期结果丢弃和视口优先绘制。
4. 验证跨页拖选、锚点 zoom 和异步过期。
5. 独立评审并复验。

## 预计改动

native/canvas、caret-ffi snapshot DTO、caret-layout 输出与窗口集成。

## 验收标准

- [ ] 页面没有独立正文或 Undo 栈；跨页命中返回同一文档位置。
- [ ] 绘制、选择、光标和 hit-testing 用同版 LayoutResult。
- [ ] 视觉左右键按 bidi/line layout 选择目标 caret；逻辑边界 affinity 只决定输入继承，混合方向文本的同一逻辑位置可由独立 visual affinity 区分。
- [ ] 命中测试将点击映射到实际命中的相邻字素簇侧；精确边界歧义按逻辑 downstream 确定。LTR 单一行内环境方向键按文档模型的边界转移表操作：向外时同一偏移切换 affinity、更新当前环境高亮/格式状态；从外侧反向移入时跨过相邻字素簇并将目标设为内侧 affinity。RTL、混排和换行处按相邻视觉 caret 候选直接移动并映射 affinity，不套用 LTR 同位置转换表。测试确认不存在无反馈的额外按键，且不以屏幕左右推断 upstream/downstream。
- [ ] hover 和拖选预览由 C++ 画布本地更新，鼠标释放时才向 Rust 提交最终插入点或选区。
- [ ] 光标进入不同嵌套环境时，高亮跟随最内层环境；块级高亮容器、行内高亮文字范围，并在退出时清除旧高亮。
- [ ] 高亮在缩放、分页和选区操作中位置正确，且不写入文档格式；环境内容仍可绘制字符高亮和底纹。
- [ ] DPR/zoom 只改变显示映射，不改文档字号和排版输入。
- [ ] 旧 generation 的异步结果不覆盖新结果。
- [ ] 对 021 提供的安全预览，未知节点在原位置显示占位和不支持能力提示；无法安全编辑的内容不可修改，可能遗漏不透明数据的整篇复制/导出不可用。
- [ ] 键盘与辅助功能能识别正文、焦点和选择。
- [ ] 独立评审和真实窗口验证通过。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-26 | `cargo test --offline --locked --workspace` | Rust 与 native 聚合测试通过 | 通过；native CTest 4/4 |
| 2026-10-26 | `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | 无 lint 警告 | 通过 |
| 2026-10-26 | `cargo clippy --offline --locked -p caret-engine -p caret-layout --all-targets -- -D warnings` | 新的 Rust 快照组合层无 lint 警告 | 通过 |
| 2026-10-26 | `cargo fmt --all -- --check`、`cargo test --offline --locked --workspace`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | 全工作区格式、测试与 lint 通过 | 通过；包含 native CTest 4/4 |
| 2026-10-26 | 画布坐标与缩放边界 C++/QML 用例 | 多页 pt 映射、hit-test 往返及缩小后的滚动范围正确 | 通过；真实窗口跨页编辑仍未实现 |
| 2026-10-26 | `cmake --build target/native/debug --parallel 4` | ViewModel 页边距解析、版心坐标映射及 QML 绑定可编译 | 通过；链接器有既存 macOS LTO 路径与 deployment-target 警告；未运行测试 |

## 风险与回退

屏幕像素与文档 pt 混算会造成 DPR 下的换行或命中漂移。转换集中实现并测试往返。

## 决策与工作记录

- 2026-10-25：从原 008 拆出画布与命中职责。
- 2026-10-26：完成首个 C++/QML 页面几何切片。Rust 空白文档页几何经 FFI 到 ViewModel；Canvas 和 ViewModel 使用独立 QML 模块。页面点坐标映射、hit-test、缩放锚点和 scene graph 纸张绘制已落地。完整 LayoutResult、文字/字素命中、selection/caret 几何、generation 丢弃、环境高亮、输入预览和真实窗口交互仍待 027 后续实现。
- 2026-10-26：新增 `caret-engine::DocumentLayoutService`，组合文档级字体偏好、段落 shaping、行布局和分页，快照携带文档/字体代际、正文原文与 shaping 结果、页尺寸/边距及段落行片段。当前不透明块、环境样式/Section 编号和带 `markStyle` 的空段或尾随软换行会明确拒绝生成快照；只接受空 kind 或 `page` 显式分页。该服务尚未连接 FFI/ViewModel，命中映射、选择/光标几何、绘制和旧代际结果丢弃仍待实现。
- 2026-10-26：初始页面快照新增 `marginsPt`；ViewModel 在完整快照安装时校验边距并提供给 Canvas。页面命中结果同时返回页面 pt 与版心相对 pt 坐标，边距未安装时不伪造版心坐标；Canvas setter 只校验自身输入，避免页面尺寸与边距分步更新时受旧值阻挡。独立复核通过；Rust/C++/QML 构建通过，尚未运行 native 测试。
- 2026-10-26：Session 新增异步布局 token：`sessionId + documentGeneration`。成功文档/页面几何命令递增 revision；选区和 IME 预编辑不递增；Session 被替换或克隆分叉时换 ID。`layout_session` 同步捕获同一 Session 的文档与 token，避免调用方误配；revision 溢出时轮换 ID 并重置计数。独立复核确认命令语义和身份隔离。
- 2026-10-26：库审查发现 Parley 0.12 高层 `Layout` 提供 caret/hit-test/selection 几何，并可设置自定义断行机会；初始原型发现它不能消费现有 `ShapedParagraph`，且跨 Run 字素有边界限制。
- 2026-10-26：受限 Parley 高层 Layout 原型验证了 ICU 断点回调、英文/中文样例行范围和普通混合 bidi 光标/命中；不同样式 Run 中的 `e + combining acute` 会暴露字素内部停靠点。该问题作为生产适配要求保留，不再据此否决高层布局。
- 2026-10-26：修正后的 10 MiB Criterion 单段落对照把双方的 ICU 断点生成、塑形和排版都计入；计时前验证逐行 UTF-8 范围完全一致。Parley 为 1.8044 s，Caret 为 2.4026 s，快约 24.9%。据此更新决策：后续统一段落布局与交互几何采用 Parley 高层 Layout，Caret 保留 ICU 断点、字体规则、行距和分页领域逻辑。该样本只覆盖单个长英文段落，不覆盖多段文档分页、内存或跨平台；实施仍需补齐跨 Run 字素、显式字体/fallback 和 selection 几何验证。先前复用预计算断点的样本因计时不对等而作废。临时对比 harness 与产物已删除；Criterion 复用 workspace 既有依赖，`target/criterion` 已忽略。`FontCatalog` 与高层布局共用同一 `FontContext`，避免重复字体扫描/缓存。
- 2026-10-26：生产段落布局已切换到 Parley 高层 `Layout`，删除自研 `TextShaper`、`CatalogSelector` 与探针；Caret 保留 ICU 断点、字体偏好、行距/分页领域逻辑。增加跨 Run/环境字素边界、字素安全光标/选区/命中、字体刷新重建 context 和 `DocumentLayoutService` 端到端测试；恢复完整 bidi levels/visual-order corpus，补回多语言 fallback、0.5/12.25 pt 字号、Emoji generic 优先、emoji/ZWJ 和相同样式 Run 保持 glyph 的测试。正文只展平一次；断点覆盖用位集存储，脚本分类线性处理，行命中二分定位。`caret-layout` 55 项、`caret-engine` 41 项，workspace 测试、Clippy、格式检查均通过，native CTest 4/4。Parley glyph runs 尚未接入 Qt scene graph/FFI，027 的端到端绘制、可视光标/选区和用户交互验收仍未完成。

## 完成摘要

未完成。
