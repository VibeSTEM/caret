// ShellLayoutSnapshotValidationTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellLayoutSnapshotValidationTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
    void commandLineProtocolDescriptionMatchesCodecBehavior();
#ifdef CARET_ENABLE_APP
    void invalidLayoutSnapshotsAreRejectedWithoutReplacingPublishedPages();
#endif
};
