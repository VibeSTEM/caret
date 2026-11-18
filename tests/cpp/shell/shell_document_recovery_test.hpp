// ShellDocumentRecoveryTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellDocumentRecoveryTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
#ifdef CARET_ENABLE_APP
    void recoveryEntriesCanBeRestoredExportedAndDiscarded();
    void missingRecoveryEntryFailsWithoutReplacingTheOpenDocument();
#endif
};
