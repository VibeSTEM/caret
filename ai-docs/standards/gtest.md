# C++ 测试（GoogleTest）

更新日期：2026-10-25。锁定版本见[技术基线](baseline.md)，获取方式见[依赖获取](dependency-acquisition.md)。目录见[测试规范](testing.md)。

## 项目规则

- 使用 `EXPECT_*` 和 `ASSERT_*`。需要补充语义时用 `<<`。后续步骤依赖的前置条件用 `ASSERT_*`。
- 套件名是被测类型，用例名是行为，都用 PascalCase。用例之间不依赖执行顺序。
- 浮点比较写明容差和单位。字符串用 `EXPECT_STREQ` 或 `EXPECT_EQ`。
- 参数化测试只用于真正的输入矩阵。death test 只用于确定会 abort 的路径。
- mock 只隔离模块边界外的依赖，并在 task 里写明边界。
- 源文件放在 `tests/cpp/<module>/`。可执行文件名为 `<被测目标>_test`。产物留在构建树，不安装。
- `BUILD_TESTING=ON` 时用 `gtest_discover_tests` 注册。关闭测试时不下载 SDK，安装树里也没有测试二进制。
- 告警选项只作用于自有 target，不放宽来迁就 GoogleTest，也不把 GoogleTest 的编译选项回灌到产品代码。

编辑器如果按真实编译数据库检查，`EXPECT_*` 展开后的断言结果依赖上下文布尔转换。误报时先生成 `compile_commands.json`，再重载编辑器。
