// ShellDocumentAutoSaveTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellDocumentAutoSaveTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
#ifdef CARET_ENABLE_APP
    void autoSaveWritesTheCurrentRevisionAfterTheDeadline();
#endif
};
