# 006 — CI 与打包管线

- 状态：implemented
- 依赖：005
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

005 之后仓库里已经有 Rust、QML 和 native 入口。本任务把来源的质量、夜间、打包和 GoogleTest SDK 工作流移植过来，并改成 caret 的名字和 `.toolchain/` 缓存。移植完成后，基础设施阶段结束。

## 必读

- [持续集成](../standards/ci.md)、[依赖获取](../standards/dependency-acquisition.md)、[测试规范](../standards/testing.md)
- [Cargo](../standards/cargo.md)

## 范围与非目标

移植 `ci.yml`、`nightly.yml`、`package.yml`、`sdk-googletest.yml` 和配套 composite action。环境变量 `PANTA_*` 改为 `CARET_*`。工具缓存指向 `.toolchain/`。

本次先检查现有代码并修复 CI 前置：补齐 CMake 分层夹具和真实构建图检查，清除测试选择器中来源项目的无效模块映射，核对质量聚合与打包入口的参数和注册。只优化影响本任务正确性或验证覆盖的代码，不做无关重构。移植基准为来源仓库 `ee84d95`，实际取用文件记录在工作记录中。按用户后续要求，日常 push/PR 只运行 Ubuntu；macOS、Windows 构建测试与 Objective-C++ 检查放入 nightly，手动打包发布仍验证三平台。

按用户补充要求，`format`、`lint`、`test` 沿用 panta 的统一调度与检查机制；GoogleTest 另参考 [sdk-packaging](https://github.com/Jack-Lab-at-EIT/sdk-packaging) `a43519510326d26e3118b5d3f888285d4c38c955` 的三平台生产、自检、归档和发布方式，在 Caret 仓库独立打包，补齐带固定摘要的懒供给消费入口。不依赖其他组织的 Release。发布默认关闭，只有发布 job 有写权限。

不移植几何、网格和渲染 SDK 的工作流。不把夜间覆盖率改成编辑器性能门禁。函数和行覆盖率门槛仍是 85%，只在夜间阻断。

## 前置条件与待决策

005 已 done，空白窗口的构建是 CI 要覆盖的 native 目标。

## 实施步骤

1. 移植 workflow 和 action，按规范改名、改缓存键。记录来源 commit。
2. 核对纯文档变更不创建运行，未知路径扩大到完整范围。
3. 本地跑通 CI 会调用的 fmt、clippy 和已有测试。workflow 本身在推送或 `gh` 手动运行后把结论写回本文件。
4. 确认没有来源前缀残留在 `.github/`。
5. 独立评审后复验。

## 预计改动

新建 `.github/workflows/`、`.github/actions/`、`tools/sdk/`、native 分层及 SDK 供给夹具、质量工具配置和 native 聚合测试；修复质量调度、native 入口异常边界及桌面 Runtime 安装部署。正文编辑行为不在本任务。

## 验收标准

- [x] 四条要移植的 workflow 都在，排除的 SDK workflow 不在。
- [x] `.github/` 里没有 `panta` / `PANTA_` 前缀。
- [x] 缓存键不包含 `target/` 里的工具目录。
- [x] 本地 fmt、clippy 和已有测试通过。远程运行若尚未推送，验证表写明未执行，不记为通过。评审发现已处理。
- [x] GoogleTest 独立生产、归档搬迁自检与供给正反夹具通过，未发布资产不伪造固定摘要。
- [x] macOS 桌面包在清除开发环境变量后可启动；Windows、Linux 远程结果另记。
- [ ] 日常 Ubuntu CI、三平台 nightly 和桌面打包有对应远程证据后，再将任务标为 done；SDK 生产发布已由 016 完成。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | 检索 `.github/` 来源前缀、`git diff HEAD --check` | 无残留、无空白错误 | 通过 |
| 2026-10-25 | `cargo quality`，macOS arm64，锁定工具链 | 跨语言格式、完整 lint、审计、workspace 测试通过 | 通过；含 4 项 CTest。审计仅有允许但当前未用到的许可证提示，无失败；不隐藏提示 |
| 2026-10-25 | 最终 SDK 事务修复后：`cargo format --check`、`cargo lint cmake --check`、`cargo test --locked -p caret-tests --test native` | 受影响检查复验 | 通过；4 项 CTest 全通过 |
| 2026-10-25 | `cargo run --locked -p caret-tests -- toolchain` | 实际使用托管工具和真实编译数据库 | 通过；不再要求没有消费者的 SDK 或不存在的手写 adapter |
| 2026-10-25 | `actionlint` 四条 workflow、`shellcheck` 五段 composite shell、本地链接检查 | 语法、脚本及文档入口可用 | 通过；65 个 Markdown 文件的本地链接与锚点通过 |
| 2026-10-25 | `cargo caret-package` 生成新目录，macOS 签名检查与清空 Qt/QML/DYLD 开发变量启动 | 安装树独立运行 | 通过；`--quit-after-load` 退出码 0，修复前缺失 QtQuick.Layouts 的情况已消除 |
| 2026-10-25 | `cargo run --locked -p caret-tests -- sdk-googletest target/sdk-production/googletest target/sdk-production/work macos-arm64` | 生产固定源码 SDK 并消费搬迁归档 | 通过；解包目录中静态链接及运行 1/1，归档在 `target/sdk-production/work/dist/` |
| 2026-10-25 | `caret_require_googletest()` 真实归档消费与失败路径夹具 | 强摘要、懒供给、CONFIG 和升级失败保留旧 SDK | 通过；真实 `GoogleTest.ConsumerSmoke` 1/1；错误摘要、缺 target、错误版本等均拒绝，旧安装保持有效 |
| 2026-10-25 | subagent `review_006`，`gpt-6.1-sol`、high，完整 diff 及最终增量 | 独立评审发现已处理并复验 | 通过；配置缺失、Windows 安装转义、manifest、发布保留资产及 SDK 升级事务均已修复，最终无剩余实质问题 |
| — | 日常 Ubuntu CI、三平台 nightly 和桌面打包 | 对应远程运行证据 | 日常 CI 待本次 PR 运行；nightly 与跨平台桌面包待运行，不把静态检查记成远程通过；SDK 另见 016 |
| 2026-10-25 | 016 的 [三平台运行](https://github.com/VibeSTEM/caret/actions/runs/40172423089)及[正式 Release](https://github.com/VibeSTEM/caret/releases/tag/sdk-googletest-1.18.0)，下载三归档复算 SHA256 | 当前仓库有完整、不可变资产 | 全部成功；正式三平台摘要固定在 `native/cmake/googletest-sha256.json`，与各校验文件和 GitHub 资产 digest 一致 |
| 2026-10-25 | 不设本地 ROOT/URL/SHA 覆盖，经默认正式 URL 和固定清单供给 SDK，再编译链接消费 | 正式发布与消费者端到端可用 | macOS 通过；`GoogleTest.ConsumerSmoke` 1/1，构建树 `target/gtest-official-consumer` |
| 2026-10-25 | subagent `final_index_review`，`gpt-6.1-sol`、high，完整暂存区及修复增量 | 无未处理实质问题 | 79 文件完整审查发现 1 项分层表达式绕过；修复后独立复跑全部分层夹具通过，原绕过被拒绝、合法 namespace 包装可用，最终无剩余实质发现 |

## 风险与回退

来源 workflow 假设的目录这阶段已经存在。若某一步仍引用排除掉的 SDK，删掉该步，不把 SDK 加回来。回退是移除 `.github/workflows` 中对应文件。

## 决策与工作记录

- 2026-10-25：创建任务。CI 放在 launcher 之后，这样第一次远程运行面对的是能编译的树。
- 2026-10-25：开始代码检查与 006 实施；工作分支 `yuki/infra-ci`。本地验证与远程 workflow 结果分开记录，006 主体尚未提交或推送；016 独立 SDK 后续获得单独提交、合并和发布授权。
- 2026-10-25：从 panta `ee84d95` 移植 CI/nightly、六个 composite action、CMake 分层检查及正反夹具、Python 锁文件与 Qt 静态分析模型；按本仓库现有 crate 和 CTest 标签裁剪。`cargo test --workspace` 现在执行 runner 自身回归测试和 native 聚合；删除不存在的几何、网格、工程基准、Miri 目标和无证据 sanitizer 豁免。
- 2026-10-25：实际 lint 发现并修复入口异常逃逸、缺失直接头文件、Cppcheck 导入字符串宏丢引号、无基准时强求 moc 目标、缺失配置文件及过期依赖审计豁免。完整质量入口包含格式、九阶段 lint、审计与 workspace 测试，未改变夜间覆盖率阈值。
- 2026-10-25：桌面包补齐静态 QML imports 扫描、Qt/Cargo 许可资料、Windows manifest 的 SDK mt 嵌入及 PE 验收、Windows 安装路径规范化。SDK 固定 GoogleTest `063de7e9578f82b369302001269680b4b1553359`、LLVM 22.1.7、C++20 和 Windows `/MD`；先完整上传 draft，已发布制品不可覆盖。
- 2026-10-25：独立评审修复 SDK 升级事务：staging 先在隔离 CXX 工程验证 CONFIG 精确版本及两个必需 targets，再写摘要和替换目录。正确摘要但 CONFIG 损坏时保留旧安装及旧标记。新增回归并由评审者独立执行夹具通过。本地聚合和受影响检查复验后标为 implemented，等待三平台远程证据。
- 2026-10-25：按用户要求收敛日常 CI 为 Ubuntu，移除 push/PR 的 macOS/Windows matrix 和 Objective-C++ job；nightly 保留三平台全量测试、sanitizer 和 macOS Objective-C++。手动 SDK 与桌面打包仍保留三平台矩阵。
- 2026-10-25：独立 SDK 已从 006 拆出为 016，并在 `main` 合并；本分支快进跟进，SDK 生产只保留 `tests/src/sdk.rs`，桌面 `package.rs` 不再重复实现。日常 Ubuntu check 与 nightly 三平台 full-tests 在成功后保存可信 main 缓存，避免 macOS/Windows 每晚冷启动；PR 不保存缓存。
- 2026-10-25：用户确认 SDK 已发布，要求更新 SHA256、对暂存区 subagent review，确认后创建 PR 合并。下载六项正式资产、逐归档复算并固定平台摘要；默认供给路径的真实消费自检通过，不再使用本机临时摘要。
- 2026-10-25：最终完整暂存区由 subagent `final_index_review`（`gpt-6.1-sol`、high）只读审查，发现分层生成表达式解析把 namespace 的冒号误作分隔符，可通过拼接 alias 绕过检查。改为只记录首个顶层分隔冒号，补 `$<LINK_ONLY:hidden::$<1:app>>` 反例后复验；其余暂存区未发现实质问题。
- 2026-10-25：最终修复增量由同一 subagent 只读复核并独立执行夹具，确认无剩余实质问题。按用户明确授权提交、创建 PR，待当前 Ubuntu CI 通过后合并；nightly 与跨平台桌面包仍保持待验收。
- 2026-10-25：按代码域拆分 PR CI 选择与工具触发：Rust package/test、C++/CMake、QML 静态检查、QML CTest 和分层检查分别选择；C++ 与 QML CTest 使用独立标签。Rust harness 普通测试改动只选择 `caret-tests`；测试选择器自身改动则验证完整路由。三个 C++ lint 工具共用一个 runner，`format` 继续按变更文件运行，不参与编译。

## 完成摘要

本地实现与验证完成，保持 panta 的统一 format/lint/test 调度机制。GoogleTest 已由 016 独立完成三平台生产、自检与正式发布，006 已固定正式摘要并跑通默认消费路径。日常 CI 只运行 Ubuntu，Windows/macOS 的完整构建测试留到 nightly；桌面包的 Windows/Linux、夜间覆盖率与动态检查仍待远程运行，不能将本任务记为 done。
