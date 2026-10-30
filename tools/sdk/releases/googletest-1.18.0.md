CI 为 macOS arm64、Linux x86_64 和 Windows x86_64 构建并自检 GoogleTest 静态
SDK。每个归档都包含上游 CMake 包、头文件、静态库、源码元数据和 BSD-3-Clause
许可证。Caret 在本仓库独立生产和发布，不依赖其他组织的 Release。消费端信任的摘要固定在仓库的 [GoogleTest 摘要清单](../../../native/cmake/googletest-sha256.json)；归档旁的 `.sha256` 文件是发布校验产物，不是运行时信任来源。

## 源码与构建

- 上游：<https://github.com/google/googletest>
- 版本：`v1.18.0`；固定 commit：`063de7e9578f82b369302001269680b4b1553359`
- 构建：Release、托管 LLVM 22.1.7、C++20、静态库；关闭 GoogleMock；Windows 使用共享 CRT。
- 自检：归档内的 `GTestConfig.cmake` 必须提供 `GTest::gtest` 和
  `GTest::gtest_main`；CI 从归档解包到独立目录后编译、链接并运行测试程序。
- 许可证：BSD-3-Clause；每个归档都包含从上游源码复制的
  `share/licenses/GoogleTest/LICENSE`。
- macOS arm64 制品以 macOS 13.0 为最低部署版本；Qt 与其他运行时依赖也须满足此基线。

Release 使用单一 tag `sdk-googletest-1.18.0`。`sdk-googletest.yml` 重新生产时，只有所有平台矩阵任务通过后才会上传完整的三平台制品集，上传完成前保持 draft。已发布的资产不可覆盖；重跑只有三个平台的摘要全部相同时才视为幂等成功，否则须升级版本或使用新的 tag。每个归档都有对应的 `.sha256` 校验文件。升级时重新核验上游源码 pin、归档内容、链接和测试结果。
