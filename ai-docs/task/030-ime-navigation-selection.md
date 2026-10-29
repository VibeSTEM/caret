# 030 — IME、键盘导航与选区交互

- 状态：done
- 依赖：022、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-27

## 目标与背景

022 已完成 Rust 编辑命令、Unicode 字素位置、选区和 composition 语义；027 已提供页面命中、caret 几何及基础键盘输入。Qt 窗口目前只接入直接文本输入、Enter、Shift+Enter 和 Backspace。此任务补齐输入法预编辑、视觉方向键导航和用户可见选区，使指针、键盘与 IME 共用 Rust Session 的文档位置和布局映射。

## 必读

- [编辑命令](../architecture/editor/commands.md)、[文字塑形](../architecture/editor/text-shaping.md)、[文稿光标与选区](../architecture/interface/caret.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)、[验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)
- 任务：[022 文本编辑、选择与 IME](022-text-editing-and-ime.md)、[027 页面画布、命中与布局快照](027-canvas-and-hit-testing.md)

## 范围与非目标

- 接入 Qt `QInputMethodEvent` 的 preedit、commit 和 cancel；preedit 以临时 Session/布局快照显示，不写入正文或增加文档 generation，commit 通过 Rust Session 恰好提交一次。预编辑文字和正文使用同一 Parley 塑形、换行及字体回退，并返回预编辑范围的下划线几何。提供 `inputMethodQuery` 所需的光标矩形、surrounding text、cursor/anchor 信息，使候选窗定位和上下文查询使用 Rust 当前快照。
- 按 [Qt 输入法事件顺序](https://doc.qt.io/qt-6/qinputmethodevent.html) 处理 `replacementStart`/`replacementLength`：替换范围相对预编辑起点，预编辑区域不计入被替换正文；空 commit 仍可携带删除范围。先将 UTF-16 相对范围转换为 Rust UTF-8 字节位置，再按 022 的扩展字素边界规则吸附，不允许把组合字符或 emoji 拆开。范围更新与 commit 为原子事务，失败保留当前正文和组字状态。
- 接入左右、上下、Home/End 等正文 caret 导航。左右按 Parley 视觉候选移动并保留逻辑 affinity；上下按目标视觉行保留期望 x；跨双向文本、软换行、段落边界和扩展字素簇的规则需有测试。
- 接入 Shift+方向键等键盘选区操作，以及鼠标拖选。拖动预览在 Qt 本地按当前布局更新，活动端光标随 focus 命中位置移动；正向拖选显示在右/下端，反向拖选显示在左/上端。释放时将最终锚点/焦点位置一次提交 Rust；Rust Session 保存已提交的逻辑选区。
- 绘制跨行、跨页选区背景与选区文字颜色，复用 Rust 布局提供的几何；焦点、代际和异步结果规则遵守 027。
- 接入复制与粘贴：Rust 按文档顺序提取纯文本选区，Qt 负责系统剪贴板读写；粘贴仍通过 Rust 的插入/替换事务提交，不在 Qt 保存第二份正文。
- 选区状态下的输入、Backspace、Enter 与软换行继续使用 Rust 命令事务。同一直接块容器内的 Body 段落选区应支持原子删除/替换和段落合并；跨不同 EnvironmentNode 容器、跨 EnvironmentTitle 与正文的选区按 020 规则拒绝且保持原状。未知字段或不兼容段落属性无法安全合并时拒绝整个事务，不丢失锚点、焦点或 preedit。

不包含撤销历史（018）、格式工具、命令模式、结构导航、协作编辑或平台特定输入法私有 API。系统输入法候选窗由 Qt 平台插件负责，不自行绘制。

## 前置条件与待决策

022 与 027 的 Rust 语义和几何可用。若 Qt 输入法事件与 Rust composition 生命周期间存在无法无损映射的通知顺序，先补适配契约和失败测试，不得在 QML 另存正文或 composition 副本。

## 实施步骤

1. 核实现有 Qt 输入事件、Rust composition/导航/选区命令和 Parley 视觉 caret API，固定 FFI DTO 与 generation 契约。
2. 将按键和 IME 事件适配放入画布输入层；鼠标点击/拖动预览与正文绘制保持分离。
3. 接通视觉导航、选区位置映射、跨页绘制和 Rust 暂态 composition 布局，验证后缀重排、输入光标位置、预编辑下划线及取消/提交原子性。
4. 用真实 QML 窗口和可构造的 Qt 输入法事件覆盖英文、中文组字、AltGr、emoji、组合字符、bidi、软换行、跨页拖选和失焦。
5. 完成候选实现后由 subagent 独立 review，修复并复验，更新 task 和架构记录。

## 预计改动

`native/canvas/document_canvas/input.cpp` 键盘与 IME 事件适配、`native/viewmodels/document/` 命令桥接和布局快照适配、`qml/Canvas/` 输入组件、`qml/PageSurface.qml` 选区叠加、`caret-engine` `editing/session/` 子模块事务、`caret-app`/`caret-ffi` 编辑入口及 C++/QML 集成测试。必要时补充架构接口和快捷键文档。不得把编辑语义迁入 Qt。

## 验收标准

- [x] Qt IME preedit 通过 Rust 暂态布局显示并绘制范围下划线，不进入正文；提交只落一次，取消不改正文。Qt 窗口事件测试验证预编辑/取消同步更新 glyph 和下划线几何，正文 generation 不变。
- [x] 输入法查询返回当前光标矩形和正确的 surrounding text/cursor/anchor；replacement 范围经 UTF-16/UTF-8 转换并按字素边界安全应用。
- [x] 左右移动遵循视觉 bidi caret；上下移动保持目标 x；所有导航停在扩展字素簇边界。
- [x] 左右与上下方向键在段首/段尾可以进入相邻段落；有选区时方向键按移动方向折叠或扩展。
- [x] Shift+方向键和鼠标拖动可生成方向稳定的 Rust 选区；拖动过程不对每个鼠标事件做 Rust 往返，活动端光标实时跟随 focus，释放时一次提交并与预览位置一致。
- [x] 选中后点击页面正文或画布空白处会取消选区；Ctrl/Command+C 按文档顺序复制文本，Ctrl/Command+V 通过 Rust 事务替换选区。
- [x] 选区高亮覆盖多行与多页并与文字绘制对齐；颜色来自 QML Theme token，Qt 不重建选区规则。
- [x] 输入、删除、Enter 和软换行对选区执行一次原子替换；编辑后位置映射正确，失败时正文与选区保持原状。
- [x] 同一直接块容器内的跨段选区删除/替换原子合并；跨环境容器、跨标题与正文、未知字段或不兼容段落合并明确拒绝且不修改文档。
- [x] 旧 layout/preedit 结果不能覆盖更新后的 Session；鼠标拖动、输入法更新和导航不触发无关整文档排版。
- [x] Rust 领域测试、FFI 测试、QML lint/format 和真实窗口集成测试通过；三平台 CI 对应工作流通过或有明确环境限制。PR 常规 CI 在 Ubuntu 执行受影响检查；macOS/Windows 完整测试由 nightly 工作流执行。
- [x] 独立 review 完成，发现项修复并复验，记录检查范围和限制。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-27 | `cargo test --locked -p caret-layout vertical_visual_movement_preserves_expected_x_and_line_edges_are_grapheme_safe` | 相邻视觉行按期望 x 移动，Home/End 目标保持字素安全 | 通过；覆盖相邻行、期望 x、视觉行尾字素边界、零步长和非有限 x；尚未接到 Session/Qt |
| 2026-10-27 | `cargo test --locked -p caret-app -p caret-engine -p caret-ffi` | composition、导航、段落边界、选区文本提取及 FFI 行为正确 | 通过；包括 Enter 后左右/上下跨段导航、拖选选区文本、IME UTF-16 替换 |
| 2026-10-27 | `cmake --build target/native/debug --target caret_shell_load_test`；`ctest --test-dir target/native/debug --output-on-failure -R '^Qml.ShellLoad$'` | Qt 输入、段落跨越、拖选、复制粘贴与空白点击清除选区可观察 | 通过；真实 QML 窗口验证了 IME 查询、左右跨段导航、鼠标拖选保持滚动位置、复制粘贴及空白点击清除选区 |
| 2026-10-27 | `cargo test --locked -p caret-app -p caret-engine -p caret-layout` | 中文跨段拖选位置合法；选区退格合并同容器段落；字素边界和软换行高亮范围正确 | 通过；覆盖中文两段拖选提交、跨段退格保留选区外文字、精确 CJK 边界及 Shift+Enter 不绘制额外尾随高亮 |
| 2026-10-27 | `cargo test --locked -p caret-engine -p caret-app -p caret-layout`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`git diff --check` | 光标交互不重复扫描长段落；跨段选区缓存有效；UTF-16 转换和分页光标位置正确 | 通过；加入两段 TextMap 有限缓存、折叠选区快速路径、分页行坐标索引和稀疏 UTF-16 检查点。未测真实窗口端到端延迟，不能据此量化实际加速 |
| 2026-10-27 | `cargo test --locked -p caret-tests --test native` | 优化后 Qt/QML 输入与原生集成流程仍可运行 | 通过；SDK、翻译加载、`Qml.ShellLoad`、`Qml.FormatCheck` 四项均通过 |
| 2026-10-27 | `cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo test --locked -p caret-tests --test native` | Rust lint 与真实 Qt/QML 集成检查通过 | Clippy 通过；原生聚合测试通过，包含 `Qml.ShellLoad`、`Qml.FormatCheck`、SDK 与翻译资源检查 |
| 2026-10-27 | subagent 独立评审 | 性能修复无缓存失效、UTF-16 边界或分页光标回归 | 修复项评审通过；范围限于本次性能改动，未代替 030 全量最终评审。未测真实窗口端到端延迟 |
| 2026-10-27 | `cargo test --locked -p caret-app -p caret-engine -p caret-ffi`；`cargo test --locked -p caret-tests --test native` | IME 插入点与文档统一排版、后续文本让位、游标与预编辑范围更新；取消恢复原文并移除装饰 | Rust 相关测试通过；真实 QML 窗口注入 `QInputMethodEvent`，验证 composition 触发布局 revision、正文 glyph 与下划线几何刷新，取消后还原；SDK、翻译、QML 加载和格式共 4/4 通过 |
| 2026-10-27 | `cargo test --locked -p caret-layout composition_underline_uses_each_mixed_script_run_and_wraps`；`cargo test --locked -p caret-app ime_preedit`；`cargo test --locked -p caret-tests --test native` | 中英混排与自动换行时，下划线跟随对应字形 run 的字体基线、偏移和粗细 | Rust 排版/应用定向测试通过；原生集成测试验证混排每段的屏幕坐标，native 聚合 4/4 通过。未在真实 macOS 输入法候选窗下目测 |
| 2026-10-27 | `cargo fmt --all -- --check`；`clang-format --dry-run --Werror`；`cargo clippy --locked --workspace --all-targets -- -D warnings` | Rust/C++ 格式与 workspace lint 正常 | 全部通过；选区遍历采用迭代器，测试中的可选值以可诊断错误处理 |
| 2026-10-27 | `cargo test --locked -p caret-app -p caret-engine ime_` | 空 commit 删除、active composition 替换范围、无效范围失败原子性及反向跨段预编辑正确 | 通过；覆盖 generation 单次递增、旧 composition 在无效范围后仍可取消，以及反向跨段预览锚定到保留段落 |
| 2026-10-27 | `cargo test --locked -p caret-tests --test native`；QML FormatCheck | 拖选从第 1 页跳至第 3 页时，第 2 页预览高亮及时出现；同页移动复用缓存 | 通过；仅按 anchor 至 focus 页间缺失范围获取命中图，同页指针更新不调用 Rust。native 4/4 通过 |
| 2026-10-27 | `cargo test --locked -p caret-app --lib`；`cargo test --locked -p caret-layout composition_underline -- --nocapture` | 选区起始处的 Qt replacement range 正确应用；预编辑失败不污染 Session；同行加入中文后英文 IME 下划线不偏移 | 通过；caret-app 86/86，caret-layout 2/2。未在真实 macOS 输入法候选窗下目测 |
| 2026-10-27 | `cargo test --workspace --locked`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo format --changed --check`；`git diff --check` | Qt replacement 先应用再安装新 preedit；空闲 IME 交互快照可安装；布局代际变化会使拖选命中缓存失效 | 通过；workspace 全部测试通过，native CTest 4/4；Clippy、格式和 diff 检查通过 |
| 2026-10-27 | `cargo test --locked -p caret-engine`；`cargo test --locked -p caret-tests --test native` | 选区在同容器跨段时原子合并；跨环境、标题/正文边界及未知字段安全拒绝；Shift 导航与鼠标正/反向拖选保留锚点/焦点方向 | 通过；caret-engine 78/78；native CTest 4/4，QML 真实窗口断言正向拖选为 0→4、反向拖选为 4→0，并断言 Shift+Left/Right 的 anchor/cursor 状态 |
| 2026-10-27 | subagent 只读 final review | 审查全部本轮 staged/unstaged 改动、030 验收、分层与 IME/选区事务 | 无 P1/P2；一条 P3 文档表述已修正为“同页复用 hit-map，未缓存页按需获取，释放时提交一次”。评审未运行测试；真实系统 IME 及三平台 CI 留待设备/PR 验证 |
| 2026-10-27 | PR #34 GitHub Actions（Ubuntu） | 受影响的 Rust、C++、QML 构建/测试及格式、Clippy、C++ lint、QML lint、CMake、分层与依赖检查通过 | 全部必需检查通过；macOS/Windows 全量测试由 nightly 执行，不属于 PR 常规 CI |
| 2026-10-27 | subagent 增量 final review | 复核 `3def337`、`3a6fd02` 的 C++ lint 修复与跨平台 include 清理 | 未发现 actionable 问题；只读审查，未运行测试 |
| 2026-10-27 | `cargo test --locked -p caret-app`；`cargo test --locked -p caret-tests --test native`；`cargo clippy --locked -p caret-app --all-targets -- -D warnings`；`cargo lint qmllint --check`；`cargo lint clang-tidy --changed --check`；`cargo lint includes --changed --check`；`cargo lint cppcheck --changed --check`；`cargo format --changed --check`；`cargo machete`；`git diff --check` | 拖选活动端光标实时预览；布局变化使旧预览失效；清除重复扫描、不可达分支和冗余刷新；检查 Rust 依赖 | caret-app 94/94、native CTest 4/4、Clippy、QML/C++ lint、格式和 diff 检查通过；cargo-machete 未发现未使用 workspace 依赖。`cargo lint --changed` 聚合包装器无法绑定 TCP listener，故逐项运行相关检查。真实 IME 候选窗未目测 |
| 2026-10-27 | `cargo test --locked -p caret-app`；`cargo clippy --locked -p caret-app --all-targets -- -D warnings`；`cargo test --locked -p caret-tests --test native`；`cargo format --changed --check`；`git diff --check` | 拆分 document view 运行时职责；拖选期间布局更新即时清除旧预览 | 原 542 行 `mod.rs` 拆为 61 行模块入口及快照、编辑、命中测试、布局缓存四个文件；Rust 94/94、Clippy 通过，native CTest 4/4，新增布局变化时清除命中缓存、高亮和 caret 预览的集成断言通过 |
| 2026-10-27 | subagent 独立 final review | 检查完整架构清理、模块拆分等价性、拖选失效和 IME 边界、死代码移除依据及规范更新 | 未发现 P1/P2/P3；23 个拆分前后函数体一致，未发现可安全删除的独立生产文件或公开 API |

## 风险与回退

视觉左右移动不能用 UTF-8 偏移增减推导；必须使用同一 Parley 布局的视觉候选。IME preedit 生命周期和文档事务分开；事件重复或迟到不能重复提交。选区几何失效时保留 Session 的逻辑选区，不以 Qt 本地状态覆盖。若某平台测试设备不可用，记录限制并用 Qt 事件注入验证适配边界。

## 决策与工作记录

- 2026-10-26：从 022 Rust 编辑语义与 027 画布几何的剩余 Qt 集成范围拆分，作为独立可验收任务。
- 2026-10-27：开始实现 030；先核对 Qt 事件适配、Rust Session 与 Parley 视觉导航接口，再按输入法、键盘/鼠标选区和绘制路径分层接入。
- 2026-10-27：`caret-layout` 新增按 Parley 当前视觉行执行上下移动并返回可复用期望 x 的原语，以及按视觉行首尾定位的字素边界校验；新增定向测试。该层尚未和 Rust Session、FFI、Qt 按键事件接通。
- 2026-10-27：导航接入 Rust Session/FFI/Qt，左右与上下在段落边界转到相邻段；拖动开始时用指针按下坐标锚定选区，拖选期间本地预览，释放后一次提交。画布空白点击会清除选区；Ctrl/Command+C 与 Ctrl/Command+V 使用 Rust 选区提取和编辑事务。
- 2026-10-27：确认多出的尾随选区块来自 Shift+Enter 的软换行：换行仍属于逻辑选区，但不再单独绘制高亮矩形。拖选命中映射只输出 Rust 字素边界；文档或字体代际在拖动中变化时废弃旧命中点。相同直接块容器内的跨段退格、替换和 Enter 会原子合并后编辑并保留两端未选文本；未知字段在提交前拒绝。
- 2026-10-27：排查方向键约 0.5 秒滞后时发现，导航提交曾为环境边界检查重新展平并分割整段文字；改为复用双段落有限 TextMap 缓存。光标几何改为快照内按行索引，空选区不再扫描页面，UTF-16 查询使用每 64 字节一个的稀疏检查点。Rust 窄验证通过；真实窗口端到端延迟仍待用户侧复测。
- 2026-10-27：IME 重叠由独立 QML `Text` 覆盖层引起，该层没有参与正文排版且与 Parley 字号/字体回退分离。现用临时 Session 与局部段落重塑生成展示快照，复用其余段落塑形结果并重算分页；UTF-16 cursor 属性映射为临时 caret，`layoutRevision` 保证正文 glyph 栅格缓存刷新。Rust 提供预编辑范围的多行几何，QML 只绘制下划线；取消清除几何并重用原正文布局。
- 2026-10-27：Architecture structure audit 的本分支改进：把选中文本抽取移入 Rust Session；按文本编辑、段落编辑、剪贴板、IME、导航和选区拆分 Rust/C++ 适配文件；统一布局/交互完整快照的安装路径；完整布局几何矩形共用严格校验器；按 Shell、画布几何、文档交互拆分原先超过 1,300 行的 Qt 集成测试源。更大范围的 `caret-doc/model.rs` 与 DSL 评估器拆分单独评估，避免在本分支扩大领域模型重构。
- 2026-10-27：最终评审发现空 commit replacement、active composition replacement、反向跨段 composition 起点和跳页拖选预览四项边界缺陷，分别补充 Rust 原子事务、保留段落映射及按需页面 hit-map 缓存，并添加回归测试。Qt/C++ 仍仅负责事件转换与绘制，段落、字素范围和替换语义归 Rust。
- 2026-10-27：第二轮独立评审发现，IME replacement 相对旧选区焦点计算，以及预编辑布局失败后留下 composition 状态的问题。提交事务现在先移除原选区，再按其折叠位置应用 Qt replacement range；预编辑先在候选 Session 上更新并完成布局预览，成功后才安装。另将接近 800 行的文档视图测试按 IME、键盘、导航和布局拆为独立文件；中文同行下划线加入高度不变回归测试。真实系统输入法画面仍需用户侧复验。
- 2026-10-27：Final review 继续补齐 Qt 同事件含 commit 与新 preedit 时的失效边界：成功 commit 会立即清除旧显示状态；若新 preedit 无效，发布已提交正文的完整快照，避免 Qt 留在旧预览。无变化的 IME 事件走交互快照安装器。拖选命中缓存新增 `layoutRevision` 校验，组字、取消或布局变化时放弃过期预览；跨页拖选集成测试同步提供该 revision。
- 2026-10-27：完成选区验收补测：Rust 逐一验证跨标题/正文、跨 Custom 容器、未知段落字段以及不兼容段落合并均原子拒绝；Qt 真实窗口验证 Shift 扩展/折叠选区及正向、反向鼠标拖选的 anchor/focus 方向。布局调用目前在 Qt 所属线程同步执行，因此不存在后台旧 layout/preedit 回调覆盖新 Session 的路径；交互快照严格匹配 document/font generation，composition 只重塑目标段落并复用其他段落，同页拖动复用 hit-map，进入未缓存页时按需获取 hit-map，释放时只提交一次逻辑选区，导航复用当前布局快照。该同步调用约束是当前实现保证，不代表未来异步布局接口已有 session-token 防护。
- 2026-10-27：拖动选区时，画布用 Rust hit-map 的活动端几何实时绘制 caret，方向反转后随 focus 移动；该预览不改 Rust 选区，也不改变 Qt 的 `ImCursorRectangle`，释放时再提交选区并清除预览。正向/反向拖选、预览清理和 IME 查询稳定性由真实 QML 窗口集成测试覆盖；独立评审未发现阻塞问题，未直接验证逐帧光栅输出或真实输入法候选窗。
- 2026-10-27：本轮结构清理将 QML 拖选代际状态合并为一个快照，统一预览重置；布局变化在指针静止时也会使拖选预览失效。Rust 命中图在一次段落查找中同时取得段落顺序，移除重复边界扫描、无效去重和由切片范围已保证不可能触发的保护分支。Qt caret 几何校验改用引用输出，并移除闪烁刷新已触发的重复绘制请求。`cargo machete` 未发现未使用 workspace 依赖；本轮没有证据支持删除独立源文件或资源。
- 2026-10-27：将 542 行 `document_view/mod.rs` 按职责拆成快照序列化、编辑命令、页面命中与布局快照管理模块，运行时状态和构造入口仍集中在模块根；拆分文件首行按仓库约定说明职责。新增真实 Qt/QML 集成断言：拖选保持静止期间布局代际变化时，焦点、选区高亮和 caret 几何立即清空，避免显示过期预览。

## 完成摘要

未完成。完成时填写 IME、视觉导航、选区交互、平台验证和独立评审结果，再与索引一起标 done。
