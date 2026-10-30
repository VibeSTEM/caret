# 测试目录与入口

更新日期：2026-10-30。断言风格见 [GTest](gtest.md)、[Rust](rust.md) 和 [QML](qml.md)。CI 范围见[持续集成](ci.md)。

`caret-tests` 已接入。下面是测试目录约定，不因目录树而提前建立空文件。现有 native 聚合入口为 `tests/integration/native.rs`，`cargo test --workspace --locked` 会同时执行 Rust 测试、qmllint 和 CTest；Qt 相关用例使用 Qt Test。GoogleTest 的生产与消费见[依赖获取](dependency-acquisition.md#googletest-生产与消费)。

## 测试设计

测试的主要目的是保护用户、调用方和其他模块可以观察到的行为：给定输入后得到的结果、公开状态变化、发出的信号/事件、持久化内容、文件或系统副作用，以及对无效输入的错误处理。测试名称应描述这些行为或契约。

- 优先从稳定边界验证完整行为。Rust 公共 API、C++/Qt 公共接口、QML 交互或 CLI 集成测试应像真实调用方一样操作并观察结果。
- 不把私有方法调用顺序、内部数据结构、缓存布局、实现所用算法或“调用了某个 helper”当作产品契约。重构内部实现后，行为不变的测试应继续通过。
- 单元测试可以覆盖私有域算法，但断言仍应描述该算法对外可见的语义结果、错误或副作用；只有当内部性质本身是明确的不变量（例如数据结构不变量、并发安全约束）时，才直接验证它，并在测试中说明原因。
- 需要验证副作用时，检查调用方真正依赖的结果，例如最终文件内容、信号负载、命令输出、状态转换或外部资源恢复；不要仅验证某个内部函数被调用。
- 按风险和契约选取正常、边界、失败与恢复场景，不要求每个函数、分支或参数组合都机械地配一条测试。不要为达到覆盖率数字添加没有行为意义的测试。
- 测试应独立、可重复，并清理创建的文件、全局状态和资源；不可依赖测试顺序、当前时间、机器语言或未声明的环境状态。
- 异步行为优先等待完成信号或可观察状态，不用固定睡眠猜时序；截图比较只用于无法用属性、文本、几何或事件验证的视觉契约。
- QML 用 Qt Quick Test / Qt Test 验证用户可见交互、属性变化、信号和组件装配行为；不要用测试内部 `id`、对象树细节或绑定执行次数代替组件契约。测试应能独立运行，异步断言使用 `tryCompare` / `SignalSpy` 等事件或条件等待。

Rust 官方指南区分可访问私有项的单元测试与通过公开 API 验证模块集成的集成测试；Qt 建议使用信号或条件等待异步结果，并优先检查对象属性而非脆弱的截图；GoogleTest 也将测试定义为给定输入并验证行为结果。参考：[Rust 测试指南](https://doc.rust-lang.org/book/ch11-00-testing.html)、[Rust 测试组织](https://doc.rust-lang.org/book/ch11-03-test-organization.html)、[Qt Test 最佳实践](https://doc.qt.io/qt-6/qttest-best-practices.html)、[GoogleTest Primer](https://google.github.io/googletest/primer.html)。

## 目录

```text
tests/
├── Cargo.toml
├── src/                         # 只放格式、lint 和范围调度
├── integration/native.rs        # 触发 CTest 和 qmllint
├── rust/<domain>.rs             # 公共 API 黑盒测试
├── cpp/<module>/*_test.cpp
├── qml/*_test.cpp
├── support/
├── fixtures/
└── performance/
    ├── rust/cpu/
    └── qt/{cpu,gpu}/
```

- 需要私有项的单元测试留在被测模块的 `#[cfg(test)]` 子模块；较大的测试集在 `module.rs` 中声明 `#[cfg(test)] mod tests;`，内容放到同级 `module/tests.rs`。这样保持私有访问与模块归属，也使覆盖率工具将测试源码排除在生产函数门槛之外。模块文件组织遵循[仓库文件结构](repository-hygiene.md#模块文件结构)。
- 公共行为放 `tests/rust/`，由所属 crate 的 `[[test]]` 显式注册。
- `integration/native.rs` 只检查构建树、qmllint 和 CTest 的退出码，不写领域断言。
- C++ 和 QML 功能测试在 `BUILD_TESTING=ON` 时注册。源文件列表写在 CMake 里，不用宽泛的 `GLOB`。
- 性能基准按 `rust|qt` 与 `cpu|gpu` 分目录；不进入默认 CTest、默认 workspace 测试或 PR/push CI，只由 nightly 手动 suite 执行。Rust 性能范围限 CPU；Qt CPU 与 Qt GPU 分别位于对应目录。源码仍参加格式检查；实际注册的手动基准目标也由 nightly 编译和运行。
- Nightly 性能 artifact 应保存 suite 输出和可比较的 Criterion `new/*.json` 统计，保留期与 runner/构建环境记录明确；Criterion HTML 报告不作为必要产物，以控制体积。artifact 供趋势诊断，不作为跨机器耗时门槛。
- 夹具放 `tests/fixtures/<module>/`。日志和覆盖率放构建树或 `artifacts/`。

## 命令

工作目录为仓库根；前置为 git、rustup 和[平台宿主能力](dependency-acquisition.md#宿主前置)。首次使用会按锁定版本供给工具，需联网。完整检查与 panta 共用相同调度方式，只裁掉不属于 Caret 的领域目标。`format`、`lint` 在 CI 中带 `--check`，省略时可能修复源码。`cargo quality` 聚合只读格式、完整 lint、依赖审计和完整 workspace 测试。

```text
cargo caret-package [新的输出目录]
cargo test --locked --workspace
cargo test-plan
cargo test-scope l1
cargo test-scope l2
cargo test-scope all
cargo verify-changed
cargo format
cargo lint
cargo audit
cargo coverage
cargo coverage native
cargo sanitize
cargo ub-check
cargo hyperfine <command> [options]
cargo performance
cargo quality
```

`cargo lint` 包含 Clippy、未使用依赖、CMake 格式与 lint、Rust/CMake 分层、qmllint、clang-tidy、include-cleaner 和 cppcheck。Objective-C++ 仅在 macOS 检查。`cargo test --locked -p caret-tests --test native` 是单独的 native 聚合；CI 可通过 `CARET_TEST_CTEST_LABELS` 选择 CTest 标签，空匹配会失败。`CARET_TEST_QML_LINT=false` 仅供已有独立 qmllint job 的 CI 跳过重复扫描，本地默认仍检查。

`l1` 是直接受影响的 Rust package，`l2` 是它们的消费者以及相关 native / QML 测试。这两个名字不是[分层](layering.md)里的层号。未知路径和公共构建边界扩大到完整范围。

`cargo hyperfine <command> [options]` 将参数转交给锁定版本的 hyperfine；没有被测命令时不调用。

日常改动用 `cargo verify-changed`。完整聚合用于范围无法缩小的验收和夜间任务。选择器默认比较工作树和 `origin/main` 的 merge-base。

Nightly 覆盖率是回归信号和最低门槛，不是测试目标：Rust 产品 crate 的全局函数和行覆盖率各至少 85%；native C++ 产品源码的全局函数和行覆盖率也各至少 85%。native 门槛排除测试源码、生成/构建产物、第三方和系统源码。Qt Quick Test 执行 QML 行为测试，并可能覆盖其调用的 C++ 产品代码；QML/JavaScript 源码不计入 C++ 覆盖率。当前仓库的锁定 Qt 测试工具没有提供 QML 源码覆盖率门槛；Qt 文档所列 QML 覆盖分析依赖单独安装的 Coco 工具，因此 QML 以行为测试和静态 `qmllint` 检查为准，不伪装成 C++ 85% 覆盖率。按 LLVM 可执行源码统计定义计算；模板实例、宏展开和行/区域映射会影响报告含义，查看报告时需结合未覆盖源码判断，不能用单个数字代替质量评审。低于门槛时应补充有行为意义的测试或修复范围配置，不得通过删除功能代码、缩窄覆盖统计范围或加入无意义断言来“刷”指标。LLVM 对函数、行、区域、分支等覆盖率的定义见[Clang 源码覆盖文档](https://clang.llvm.org/docs/SourceBasedCodeCoverage.html)；Qt 的测试框架说明见[Qt Test 最佳实践](https://doc.qt.io/qt-6/qttest-best-practices.html)和[Qt Quick Test](https://doc.qt.io/qt-6/qtquicktest-index.html)。

覆盖率 artifact 保存原始汇总、产品源码报告和详细 HTML 供诊断；nightly 门禁以过滤后的 native 产品源码函数与行汇总为准。性能数字写明输入、机器、配置和测量方式；没有数据时标成待验证。编辑器的帧时间和排版预算写在架构文档里，不设进这条 CI 门槛。

迁移测试时同时改 CMake、Cargo manifest 和文档，并清掉旧路径。同一用例不留两份。
