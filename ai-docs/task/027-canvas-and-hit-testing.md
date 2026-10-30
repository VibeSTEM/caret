# 027 — 页面画布、命中与布局快照

- 状态：done
- 依赖：022、024、026
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-27

## 目标与背景

以单一 LayoutResult 绘制连续多页，并使文字、页面、插入点、选区和命中测试共用同一映射。

当前已完成页面几何切片、Session 布局代际、Parley 段落布局，以及 Rust 页面局部命中、插入点和跨页选区几何。Rust 页面 glyph 数据经 CXX 二进制 DTO 到 Qt，Canvas 使用 Qt 管理的纹理图集绘制；行框与页面裁切及设备像素相位已接通，当前视口内所有页面按需请求。插入点已通过主题 token 在 Qt scene graph 呈现。基础键盘输入已接入 Rust Session：文本输入、Enter 拆段、Shift+Enter 软换行、Backspace 字素删除与段首合并；IME、键盘导航和选区交互由 030 承接。027 剩余真实文档文字的完整窗口验收。环境高亮和未知节点安全预览分别由 031、032 承接。正文渲染采用 Parley glyph ID 与位置；Rust 使用 Swash 按同一字体面栅格化，避免 Qt 重新塑形或重新匹配字体。共享行距网格由 033 验收；OpenType BASE 脚本基线由 055 后续处理，不作为 027 的验收范围。

## 必读

- [界面外壳](../architecture/interface/ui-shell.md)、[页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)
- [光标与选区职责](../architecture/interface/caret.md)
- [增量排版](../architecture/editor/incremental-layout.md)、[分层](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

实现滚动工作区、纸张绘制、布局快照、文本命中、选区/光标几何和 zoom。异步结果携带文档 generation，过期结果不能覆盖新版本。跨页选区仍是一条逻辑选区。本任务提供选区几何和布局命中能力；键盘导航、拖选交互及选区呈现由 030 验收；当前环境背景高亮由 031 验收；未知节点安全预览由 032 验收。

光标与选区状态由 Rust 提供，页面内几何由 LayoutResult 提供，Canvas 只绘制。纸张、边框、光标和选区颜色来自 `qml/Themes/` token 并由 QML 注入；闪烁、粗细和焦点状态等视觉细节以[独立设计](../architecture/interface/caret.md)为准。

不包含复杂尺规/页眉页脚、增量排版缓存和性能门槛；基准留给 013。页面几何使用前缀索引和 scene graph 几何，避免全篇纹理和二次方页面扫描。

## 前置条件与待决策

022 输入位置、024 glyph cluster、026 page layout 结构稳定；明确 C++ 画布、FFI 和 Rust 视图边界。

## 实施步骤

1. 定义 LayoutResult 到画布快照的最小 DTO。
2. 统一页面变换、命中、选区和绘制坐标。
3. 接入 generation 过期结果丢弃和视口优先绘制。
4. 验证跨页选区几何、锚点 zoom 和异步过期；不在本任务接入键盘导航或拖选交互。
5. 独立评审并复验。

## 预计改动

native/canvas、caret-ffi snapshot DTO、caret-layout 输出与窗口集成。

## 验收标准

- [x] 页面没有独立正文或 Undo 栈；跨页命中返回同一文档位置。
- [x] 绘制、选择、光标和 hit-testing 用同版 LayoutResult。
- [x] 命中测试将点击映射到实际命中的相邻字素簇侧；精确边界歧义按逻辑 downstream 确定。逻辑和视觉亲和性分离，不以屏幕左右推断 upstream/downstream；完整视觉方向键规则由 030 验收。
- [x] DPR/zoom 只改变显示映射，不改文档字号和排版输入。
- [x] 旧 generation 的异步结果不覆盖新结果。
- [x] 插入点仅在画布获得活动焦点时显示，失焦时隐藏；焦点或 Rust 插入点变化时立即显示，按 Qt 平台周期闪烁。`caretBlinkEnabled=false` 时常亮，平台周期为 0 时也常亮；视图状态不复制 Rust 文档或选区状态。
- [x] 页面命中与布局快照为后续输入提供稳定的位置、caret 和选区几何 API。基础文本输入及 Enter/Backspace 编辑已接入；IME、方向键导航、拖选交互和选区绘制由 030 验收。
- [x] 滚动/几何变化在当前事件循环合并刷新；文档编辑不等待定时器，最新 glyph frame 在输入后尽快可见。旧帧不能让 caret 超前于正文，最新 generation 持续可见。
- [x] 通过真实窗口验证 Rust 文档输入、布局、glyph 栅格化和 Qt Canvas 绘制形成完整链路。
- [x] 独立评审和真实窗口验证通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout -p caret-engine` | 段落与文档布局、跨页选区/命中回归通过 | 通过；caret-layout 61 项、caret-engine 42 项 |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout -p caret-engine` | 编辑/布局与 glyph snapshot 回归通过 | 通过；caret-engine 54 项、caret-layout 68 项 |
| 2026-10-26 | `cmake --build target/native/debug --target caret_shell_load_test -j2`、`ctest --test-dir target/native/debug -R '^Qml\.ShellLoad$' --output-on-failure` | 真实窗口可见 Rust 输入 glyph；插入全角 `！` 后最新 generation、glyph placement 和实际窗口像素更新；Enter/Backspace/软换行继续工作 | 通过；`Qml.ShellLoad` 1/1；构建只有既存 macOS LTO/deployment-target 警告 |
| 2026-10-26 | `cargo clippy --offline --locked -p caret-layout -p caret-engine --all-targets -- -D warnings`、`cargo format --changed --check`、`git diff --check` | Rust lint、变更格式和补丁空白检查通过 | 通过；clang-format 已修正真实窗口测试中的 8 处格式差异 |
| 2026-10-26 | subagent 只读独立评审完整相关 diff | 画布 glyph 生命周期、像素断言、任务拆分与架构行距规则无高/中风险问题 | 通过；修复了全角 glyph 像素断言、任务归属引用和行距规则遗漏后复核通过 |
| 2026-10-26 | `cmake --build target/native/debug --target caret_shell_load_test -j2`、`ctest --test-dir target/native/debug -R '^Qml.ShellLoad$' --output-on-failure` | 键入后 glyph frame 在事件循环合并刷新中更新；Enter/Backspace/软换行仍工作 | 通过；单字符本地窗口用例中，Rust/layout generation 更新与 glyph frame 属性均在按键事件返回前可观察 |
| 2026-10-26 | `cargo fmt --all -- --check` | Rust 格式符合规范 | 通过 |
| 2026-10-26 | `cargo clippy --offline --locked -p caret-engine -p caret-layout --all-targets -- -D warnings` | 新的 Rust 快照组合层无 lint 警告 | 通过 |
| 2026-10-26 | `cargo test --offline --locked --workspace`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | 全工作区测试和 lint 通过 | 通过；native CTest 4/4 |
| 2026-10-26 | 画布坐标与缩放边界 C++/QML 用例 | 多页 pt 映射、hit-test 往返及缩小后的滚动范围正确 | 通过；真实窗口键盘编辑链路仍在补齐 |
| 2026-10-26 | `cmake --build target/native/debug --parallel 4` | ViewModel 页边距解析、版心坐标映射及 QML 绑定可编译 | 通过；链接器有既存 macOS LTO 路径与 deployment-target 警告；未运行测试 |
| 2026-10-26 | 独立复核分页选区 y 偏移、页末命中和命中 affinity | 分页片段选区、页末命中和 RTL/LTR 字素边缘映射正确 | 通过；多行 bidi、硬换行和 LTR/RTL 起始边缘问题均修复并复验 |
| 2026-10-26 | `cargo test --workspace --locked`、`cargo format --changed --check`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | Rust、C++/QML、格式和 lint 验证通过 | 通过；workspace/native 聚合测试及 CTest 4/4 通过；clippy 与变更文件格式检查通过 |
| 2026-10-26 | 独立复核布局 JSON 数值边界和结构校验 | 拒绝不精确代际、越界字节偏移和非法行/片段几何，错误快照不替换已安装快照 | 已按复核修复；新增 JSON→C++→QML 全 `u64` 代际、UTF-8 边界和非法行框回归用例 |
| 2026-10-26 | `cargo test --locked --workspace`、`cargo format --changed --check`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | 持久 FFI 文档运行时可编译，Rust、C++/QML、格式和 lint 验证通过 | 通过；workspace/native 聚合测试及 CTest 4/4 通过；格式与 Clippy 通过 |
| 2026-10-26 | 独立评审持久 FFI 运行时的生命周期、分层与错误边界 | C++ Box 析构、同步 UI 线程调用和会话缓存生命周期明确 | 无阻塞问题；按建议补充所有权、有效期、线程和 UTF-8 错误约定 |
| 2026-10-26 | `cargo test --locked --workspace`：Rust 页面命中 → FFI JSON → ViewModel → QML `TapHandler` | 点击页面返回稳定故事、段落 ID、UTF-8 偏移、逻辑/视觉 affinity 和文档 generation；页面外不产生命中 | 通过；C++/QML 用例验证实际窗口点击发出位置事件，页面外命中无效，CTest 4/4 |
| 2026-10-26 | 独立复核本次完整 diff 与真实 QML 点击链路 | 查缓存、所有权、分层、坐标与返回值边界 | 无阻塞问题；复核者独立重跑 `Qml.ShellLoad` 通过；跨页/缩放和双向文本端到端输入仍待覆盖 |
| 2026-10-26 | Rust 页面点选更新 Session 折叠选区与独立 visual affinity | 命中后提交稳定 UTF-8 位置和视觉侧；miss 不改状态，选区变更不递增文档 generation | 通过；`caret-engine` 验证视觉侧与逻辑 Pos 分离、失败原子性和 Session fork；`caret-app` 验证页面点选；workspace CTest 4/4 |
| 2026-10-26 | 独立复核页面点选、视觉 affinity 与 Effect 语义 | 检查失败原子性、状态重置、fork、缓存代际和 FFI/ViewModel 分层 | 无阻塞问题；复核者独立重跑 `caret-engine` 与 `caret-app` 定向用例通过 |
| 2026-10-26 | `cargo test --offline --locked -p caret-engine -p caret-app`、`cargo fmt --all -- --check`、`cargo clippy --offline --locked -p caret-engine -p caret-app --all-targets -- -D warnings`、`git diff --check` | 页面命中同时返回与命中位置一致的页面局部插入点几何，分页片段坐标及 JSON DTO 正确 | 通过；`caret-engine` 43 项、`caret-app` 52 项；首/中/末页及 JSON 字段均有断言 |
| 2026-10-26 | 独立复核页面插入点几何与 View DTO | 检查 caret x 坐标、分页片段 y 偏移、行高、命中位置一致性及分层 | 无阻塞；发现段落全局行顶差分与分页局部累计存在约 0.11 pt 漂移，改为复用命中行的分页局部累计值；复核者在 200,000 行样本末 50 页抽查偏差为 0，`caret-engine` 43 项与 `caret-app` 52 项独立复跑通过 |
| 2026-10-26 | `cmake --build target/native/debug --parallel 4`、`ctest --test-dir target/native/debug -R Qml.ShellLoad --output-on-failure` | caret 主题绑定、scene graph 页面坐标、超高行页面裁切及非法几何拒绝符合预期 | 通过；真实窗口像素验证了 caret 绘制、纸张外裁切，0.5 和超范围 pageIndex 不会覆盖当前几何 |
| 2026-10-26 | 独立复核 QSG caret 绘制、主题与输入边界 | 检查分页/缩放坐标、超高行裁切、scene graph 节点生命周期、主题 token 和无效输入 | 无阻塞；独立复跑 native 定向测试通过。焦点/失焦、闪烁和高 DPR 人工验收待后续 |
| 2026-10-26 | `cargo format`、`cargo test --locked -p caret-tests --test native` | 焦点显示/失焦隐藏、闪烁开关、平台周期为 0 与周期动态变化符合预期 | 通过；真实 Qt 窗口像素断言覆盖开关和周期变化，CTest 4/4 |
| 2026-10-26 | 独立复核画布焦点和光标闪烁 | 检查 Qt 活动焦点、系统周期动态变化、开关状态及 scene graph 更新 | 无阻塞；确认窗口失活时 activeFocus 沿 Qt 焦点作用域变化；开关、关闭和恢复周期回归测试通过 |
| 2026-10-26 | `cargo test --offline --locked --workspace`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 全工作区 Rust、native CTest、QML 格式和静态检查通过 | 通过；native CTest 4/4，workspace Clippy 无警告 |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout`、`cargo clippy --offline --locked -p caret-layout --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 精确字体面栅格化、TTC face 缓存隔离、合成斜体、彩色 alpha、位图亚像素放置和缓存复用符合预期 | 通过；caret-layout 67 项。macOS TTC face 0/1 输出与独立 rasterizer 一致；Apple Color Emoji 位图相位由调用方保留，Apple Symbols 轮廓字形设备 y 质心按 +0.25 px 移动。Qt 图集绘制尚未接入 |
| 2026-10-26 | 独立评审 Swash 字体面、alpha 混合、合成样式、缓存键及设备像素相位 | 不同 TTC face 不串缓存；轮廓与位图在 Qt 合成时保留正确颜色和设备坐标 | 无阻塞；评审期间发现并修复预乘 alpha、synthetic italic 轴向、TTC face 缓存键和设备 y 相位问题；独立重跑 5 项栅格化定向测试通过。实际 QSG atlas 合成验收仍待后续 |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout -p caret-engine`、`cargo fmt --all -- --check`、`cargo clippy --offline --locked -p caret-layout -p caret-engine --all-targets -- -D warnings`、`git diff --check` | 页面 glyph 快照保留准确字体面、glyph ID、视觉顺序、variation/synthesis、页局部坐标和 Exactly 行框裁切；超长段落组合附标不丢失局部偏移 | 通过；caret-layout 68 项、caret-engine 49 项。独立复核无阻塞；Qt atlas 绘制和真实窗口像素验收仍待后续 |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout -p caret-engine -p caret-app -p caret-ffi`、workspace 定向 Clippy/fmt、`cmake --build target/native/debug --parallel 4`、`ctest --test-dir target/native/debug --output-on-failure` | CXX 二进制像素 DTO、当前页 atlas 绘制、设备像素放置和 Exactly 行框裁切在 Qt 窗口中工作 | 通过；Rust 相关 crate 全部测试通过，native CTest 4/4；QQuickWindow 像素测试确认行框外像素被裁切。该窗口测试用合成图像验证 QSG 合成；真实字体从 Parley 到窗口的端到端像素比对仍待后续 |

## 风险与回退

屏幕像素与文档 pt 混算会造成 DPR 下的换行或命中漂移。转换集中实现并测试往返。

## 决策与工作记录

- 2026-10-25：从原 008 拆出画布与命中职责。
- 2026-10-26：完成首个 C++/QML 页面几何切片。Rust 空白文档页几何经 FFI 到 ViewModel；Canvas 和 ViewModel 使用独立 QML 模块。页面点坐标映射、hit-test、缩放锚点和 scene graph 纸张绘制已落地。真实窗口跨页编辑仍未实现。
- 2026-10-26：Session 布局代际与 `DocumentLayoutService` 已实现；服务生成包含正文文本、段落布局、分页和字体代际的只读快照。未知块及暂不支持的环境样式会拒绝整份布局，尚未连接 FFI/ViewModel。
- 2026-10-26：段落布局改用 Parley 高层 Layout；Caret 保留 ICU 断点、字体偏好、行距和分页。一次 10 MiB 英文长段落对照中，双方计入断点生成、塑形和排版后，Parley 为 1.8044 秒，原实现为 2.4026 秒。该样本仅用于实现选型，不代表多段分页、内存或跨平台性能结论。
- 2026-10-26：`DocumentLayoutSnapshot` 增加页面局部命中、跨页选区矩形及会话/文档/字体代际校验。独立复核发现并修复分页片段选区 y 偏移、固定/自然行高导致的页末误命中、小字号下光标矩形造成的命中偏差，以及按 Parley 命中字素侧生成逻辑 affinity。新增 0.5/12 pt 的 LTR、RTL、bidi 与跨 Run combining mark 回归测试。复核另发现 Parley 多行 bidi 光标几何错误行问题，未纳入本次修复。
- 2026-10-26：后续复核确认 Parley 自身在多行 bidi 的 `Cursor::geometry` 可能选错行；适配层现按逻辑 affinity 对应的相邻字素簇计算 caret 几何，并新增第二行命中与光标几何一致性测试。补充处理硬换行后的下一行及尾随空行光标位置，覆盖 LF、CRLF、连续换行和 RTL。独立复核通过；`caret-layout` 61 项、`caret-engine` 42 项，格式、Clippy 和定向测试通过。
- 2026-10-26：应用层新增会话级布局运行时，持有系统字体目录和可复用布局器；FFI 传递携带 session/document/font generation、页面片段及段落 UTF-8 行范围的只读 JSON，ViewModel 校验并暴露页面、段落和代际属性。generation 以十进制字符串通过 JSON 和 QML，避免 JavaScript 双精度舍入；ViewModel 拒绝无效 UTF-8 偏移、行范围、重复段落 ID、非连续行框、负度量及超出版心的片段，失败快照不覆盖已安装结果。glyph 数据、Qt 绘制及异步旧代际丢弃仍未接入。
- 2026-10-26：FFI 新增不透明 Rust 文档运行时句柄，由桌面入口持有到窗口退出，复用 Session、系统字体目录和布局缓存；初始快照由该句柄生成。后续命令与异步布局将接在同一运行时生命周期上。glyph 数据、Qt 绘制及异步旧代际丢弃仍未接入。
- 2026-10-26：命中请求复用上述运行时的当前布局快照，返回十进制字符串形式的文档 generation 和 UTF-8 偏移、稳定故事/段落 ID、逻辑及视觉 affinity。ViewModel 限制同步调用在 UI 线程；页面 TapHandler 在释放时调用 Rust，将点选位置和独立 visual affinity 一起写入 Session 的折叠选区，再发出 `documentPositionHit` 信号。视觉侧不塞进逻辑 Pos；其他会改变焦点位置的命令按新逻辑位置重置视觉侧。选区变更不递增文档 generation；命令编辑、glyph 绘制、光标/选区呈现及环境高亮仍未接入。
- 2026-10-26：页面命中结果现在携带同一 LayoutResult 计算的插入点 x 和行框 y 范围，按页局部 pt 坐标返回给 FFI/ViewModel；光标宽度、颜色与闪烁仍由界面主题和后续呈现逻辑决定。首末页坐标及 JSON 投影已加入回归断言，Qt 绘制尚未接入。
- 2026-10-26：长文档独立复核发现段落全局 `line_box_tops_pt` 差分与命中时按页面片段累加会产生约 0.11 pt 漂移。插入点 y 现复用命中循环的片段局部行顶，200,000 行样本末 50 页抽查偏差为 0；Rust/Qt 分层和 DTO 投影复核无其他阻塞问题。
- 2026-10-26：`DocumentCanvas` 接收 Rust 页面局部 caret DTO，使用 QSG 矩形节点绘制插入点；光标颜色和整数厘像素宽度由 Rust theme schema 校验后经 QML 传入。Caret 绘制按纸张范围裁切，超高行可显示页内部分；pageIndex 严格校验，非法更新不会替换当前几何。QQuickWindow 测试覆盖缩放坐标、底边裁切、焦点显隐和闪烁。页面空白区点击、文本输入和 Rust 编辑后的重排刷新已接通。
- 2026-10-26：选择正文渲染路径为 Parley glyph ID/位置 → Swash 同字体面栅格化 → Qt scene graph 纹理图集。Qt 的 QRawFont 公共构造接口不能精确指定字体集合索引，也不能完整表达 Parley 的 variation/synthesis；按字体族重新匹配无法保证 glyph ID 一致。`caret-layout` 已有栅格化器，键包含 Blob 唯一 ID、集合索引、glyph ID、字号、轴坐标、合成提示、亚像素相位和前景色；图片缓存上限 16 MiB、8192 项，单字形上限 4096 px/em。Swash 输出统一为预乘 RGBA8；alpha mask 使用前景色着色，彩色位图转换为预乘格式，COLR 前景层采用 run 颜色。设备坐标的 y 相位转换为字体坐标时反向；结果标明位图源未应用相位，Qt 绘制必须保留对应小数放置。按可见页请求和 Qt 图集绘制尚未实现；后续需明确不支持字形/颜色来源时向界面返回可见错误，不能静默略过。
- 2026-10-26：`DocumentLayoutSnapshot::page_glyphs()` 只访问页面片段引用的 Parley 行，按视觉顺序提取字体 Blob、集合索引、variation 坐标、synthesis、glyph ID 与页面局部 pt 坐标。长段落坐标逐行在 f64 累加，并把局部 glyph y 在 f64 中加入，避免组合附标受全局 f32 baseline 精度影响。Exactly 行距同时传出逐行裁切 y 范围，Natural 不额外裁切；非法页面、片段或字形源整页拒绝，inline box 返回明确错误。页面 glyph 的 FFI、栅格缓存接入运行时和 QSG atlas 绘制尚未完成。
- 2026-10-26：应用运行时持有有界 `PageGlyphRasterizer`，只对请求页生成 RGBA 字节与 placement；页内先按 Swash 使用的完整稳定 raster key 去重，确保全局缓存淘汰不破坏索引，也不额外 pin 整页原图。CXX 传递 `Vec<u8>` 和 placement DTO，不将像素编码到 JSON；ViewModel 校验像素长度、索引、相位和行框范围后转换为 QByteArray。Canvas 只创建一次当前帧纹理，允许 Qt 将小 glyph 放入 atlas，并在 scene graph 线程构造图像节点；字形矩形按页面与行框裁切后同步缩小 atlas source rect。QML 以 40ms debounce 请求 viewport 中心页，缩放、滚动、窗口移动和 DPR 变化后按新的设备像素相位刷新。Qt 窗口像素测试覆盖 atlas-backed image node 与 Exactly 行框裁切；真实 Parley 字体端到端像素对照、滚动大页耗时和异步栅格化仍待后续。
- 2026-10-26：渲染复核修正 `QSGImageNode::setSourceRect` 坐标：调用方传纹理像素坐标，由 Qt 负责 atlas 子矩形映射，避免重复归一化。双色合成像素测试验证裁切后仍采样到正确颜色。QML 用页 y 范围二分查找可见页，再只迭代视口相交页，避免低缩放多页视口缺字或遍历整份文档；QSG 按页保留纹理和节点，只有帧实际变化的页面重建纹理，居中平移只更新 glyph 节点几何。像素测试覆盖两页同时绘制、只更新第二页、第一页面纹理视觉保持、宽度居中与缩放旧帧临时适配。颜色、DPR、缩放、窗口设备相位进入刷新路径。当前 QML 仍请求全部可见页并在 UI 线程同步栅格化；更细的行/字形脏区需要 Rust 布局层提供受影响页范围，避免分页重排造成漏绘。QML raster 调用经 40ms debounce；真实 Parley 字体到窗口的端到端比对与大页性能采样待后续。
- 2026-10-26：输入延迟复核发现固定间隔刷新会让编辑正文等待，也不能证明 UI 线程栅格耗时可接受。已移除 40ms QML Timer；布局、滚动、缩放和主题事件通过 `Qt.callLater` 与 pending 标志在当前事件循环合并，空闲时不轮询。窗口测试验证输入后最新 glyph frame 可见；真实长文档栅格耗时仍需按 013 的性能计划测量。
- 2026-10-26：`cargo test --offline --locked -p caret-app visible_page_rasterization_returns_binary_glyph_images`、`cmake --build target/native/debug --target caret_shell_load_test -j2`、`ctest --test-dir target/native/debug -R '^Qml\\.ShellLoad$' --output-on-failure` 均通过。Rust 定向测试与真实 Qt 窗口用例覆盖 glyph 栅格化、输入后的文档 generation 和非空 glyph placements；未做真实字体像素截图对照。
- 2026-10-26：`cargo test --offline --locked -p caret-layout -p caret-engine` 通过；caret-layout 68 项、caret-engine 54 项。全角中文 `！`、混排和阿拉伯用例验证了当时的全局锚点实现；脚本基线与稳定行推进现拆至 033，这些测试不构成 033 的验收证据。
- 2026-10-26：真实 Qt 窗口测试新增 glyph ink 像素检查，并在 Rust 文档中插入全角 `！` 后确认画布展示新代际字形；定向 CTest、Rust 布局/引擎测试、Clippy、变更格式与 diff 检查通过。脚本基线与大字号/回退字体行框溢出明确交由 033，不阻塞 027。
- 2026-10-30：此前记录中交由 033 的“脚本基线与大字号/回退字体溢出”现拆分处理：033 仅负责共享行距网格与选区行框几何，OpenType BASE 对齐及 ink 溢出诊断由 055 承接。
- 2026-10-27：029 将 `DocumentCanvas` 按 scene graph、glyph frames、几何/命中、caret 和输入拆到 `native/canvas/document_canvas/`，保留单一 `caret_canvas` target 与原 QML API。缩放范围/步进归 `CanvasPolicy`，pt/逻辑像素、centipixels 和百分比/倍率换算归 `DocumentUnits`；可见页边界直接使用实际 zoom 的 `documentToContent()`，不再经整数百分比近似。此结构调整不重开 027 的产品验收；本次集成 CMake/CTest 与 workspace 结果记在 029。

## 完成摘要

027 已完成 Rust 页面布局与 glyph 生成、Swash 栅格化、Qt Canvas 图集绘制、页面命中/插入点/选区几何、焦点与闪烁、基础键盘编辑和事件驱动的画布更新。真实窗口测试确认 Rust 文档插入英文字符和全角 `！` 后，新 glyph 可见于 Qt 窗口像素。定向 Rust 测试、Clippy、格式检查、CTest 与独立评审通过。IME/导航/完整选区交互由 030 承接；环境高亮和未知节点安全预览由 031/032 承接；共享行距网格由 033 完成，OpenType BASE 脚本基线与字形溢出诊断由 055 承接，不属于 027。
