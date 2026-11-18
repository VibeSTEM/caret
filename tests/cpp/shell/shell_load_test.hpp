// ShellLoadTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellLoadTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
    void moduleImportsAndThemeBindingTracksSnapshot();
    void builtinCodeFontIsAvailableToQt();
    void autoScrollTextMovesOnlyWhenActiveAndOverflowing();
    void autoScrollTextRepeatsWithTheDocumentedPauseCycle();
    void settingsDialogDraftCancelAndFailure();
    void unknownIconRefusesColoring();
    void documentModeSelectorTracksLocalChoice();
    void newDocumentDialogHasNoProcessFields();
    void documentUnitsConvertThroughIntegerCentipixels();
#ifdef CARET_ENABLE_APP
    void preferencesWithoutRuntimeRejectChangesAndKeepDefaults();
    void settingsDialogCommitsPreferencesThroughRust();
    void preferencesPersistAndRefreshUserFacingSwitches();
    void preferencesKeepTheLastSnapshotWhenPersistenceIsUnavailable();
    void settingsSelectionUpdatesThemeLanguageAndPersistsAtomically();
    void dialogSizesTrackLocalizedAndOptionalContent();
#endif
};
