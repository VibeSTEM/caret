// Qt/QML Shell 集成测试的用例声明与共享测试数据接口。
#pragma once

#include <QColor>
#include <QObject>
#include <QString>
#include <QVariantMap>

namespace caret::shell_test {

QVariantMap snapshot_from_theme_facade(const QString& path, const QColor& background, int body);

} // namespace caret::shell_test

class ShellLoadTest : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void moduleImportsAndThemeBindingTracksSnapshot();
    void autoScrollTextMovesOnlyWhenActiveAndOverflowing();
    void autoScrollTextRepeatsWithTheDocumentedPauseCycle();
    void unknownIconRefusesColoring();
    void documentModeSelectorTracksLocalChoice();
    void newDocumentDialogHasNoProcessFields();
    void unsavedChangesDialogUsesStyledActionsInRequestedOrder();
    void closeFailureDoesNotReopenPendingConfirmation();
    void documentCanvasMapsPageCoordinatesAndHitsPages();
    void documentUnitsConvertThroughIntegerCentipixels();
    void canvasPolicyPublishesDocumentedZoomLimits();
    void documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation();
    void windowChromeMetricsTracksWindowLifecycleAndRefreshEvents();
    void settingsDialogDraftCancelAndFailure();
#ifdef CARET_ENABLE_APP
    void preferencesWithoutRuntimeRejectChangesAndKeepDefaults();
    void blockEnvironmentHighlightSnapshotReplacesAndClearsRects();
    void commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument();
    void documentCanvasRoutesCommandKeysAndKeepsNormalEditingBehavior();
    void themeCatalogInstallsTypedTokensAndRejectsInvalidSnapshotsAtomically();
    void settingsDialogCommitsPreferencesThroughRust();
    void preferencesPersistAndRefreshUserFacingSwitches();
    void preferencesKeepTheLastSnapshotWhenPersistenceIsUnavailable();
    void settingsSelectionUpdatesThemeLanguageAndPersistsAtomically();
    void dialogSizesTrackLocalizedAndOptionalContent();
#endif
#ifdef CARET_ENABLE_APP
    void documentHitTestReturnsRustPosition();
    void documentInputDeltaKeepsUntouchedPagesAndParagraphs();
    void documentFileFlowPreservesSaveStateAndReopens();
    void autoSaveWritesTheCurrentRevisionAfterTheDeadline();
    void saveConflictCannotReplaceAPreviousDocumentAfterSaveAs();
    void recoveryEntriesCanBeRestoredExportedAndDiscarded();
    void documentHistoryUndoRedoTracksRustSession();
    void documentCaretNavigationUpdatesObservablePosition();
    void confirmedConflictReplacementWritesCurrentDocument();
    void invalidLayoutSnapshotsAreRejectedWithoutReplacingPublishedPages();
    void missingRecoveryEntryFailsWithoutReplacingTheOpenDocument();
#endif
};
