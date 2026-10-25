// 验证打包 SDK 能提供头文件、静态库并执行 GoogleTest 用例。

#include <gtest/gtest.h>

TEST(GoogleTestSdk, LinksAndRuns) {
  EXPECT_EQ(2 + 2, 4);
}
