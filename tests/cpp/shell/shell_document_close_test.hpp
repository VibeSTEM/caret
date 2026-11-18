// ShellDocumentCloseTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellDocumentCloseTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
#ifdef CARET_ENABLE_APP
    void closingAnUneditedWindowClosesImmediately();
    void cancellingEditedDocumentCloseKeepsChanges();
    void discardingEditedDocumentClosesWithoutSaving();
    void savingEditedDocumentBeforeClosePersistsChanges();
    void unnamedDocumentSaveAsContinuesCloseAndCancelCanRetry();
    void autoSaveFlushCompletesBeforeClosing();
    void autoSaveFailureReopensCloseConfirmation();
    void saveConflictKeepsWindowAndRecoveryCopy();
    void recoveryWriteFailureReopensCloseConfirmation();
    void closeRequestDuringFailedSaveDoesNotReopenConfirmation();
    void unrelatedFileFailuresDoNotCorruptCloseSaveState();
#endif
};
