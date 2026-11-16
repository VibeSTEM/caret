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
    void builtinCodeFontIsAvailableToQt();
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
    void preeditCancellationDoesNotReenterTheInputMethodHandler();
    void mathSourceDisablesInputMethodAndCancelsPreedit();
    void documentPathAdapterCreatesDefaultSaveLocation();
    void documentPathAdapterConvertsLocalPaths();
    void windowChromeMetricsTracksWindowLifecycleAndRefreshEvents();
    void settingsDialogDraftCancelAndFailure();
#ifdef CARET_ENABLE_APP
    void preferencesWithoutRuntimeRejectChangesAndKeepDefaults();
    void environmentHighlightSnapshotSelectsAndClearsActiveRects();
    void commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument();
    void inputMethodContextFlowsFromViewModelToCanvas();
    void cancelledInputMethodStateAllowsTheNextCommit();
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
    void documentCanvasClickAndTypingPublishRustSnapshot();
    void documentCanvasRasterizesTypedGlyphWithInk();
    void documentFontFallbackPreservesLatinBaseline();
    void returnSplitsParagraphAndForwardsEditorKey();
    void inputMethodCompositionUnderlineTracksPageTransforms();
    void mixedCompositionPublishesOneUnderlinePerRect();
    void cancelledCompositionClearsPreeditAndCompositionRects();
    void escapeClosesCommandLineWithoutEditingTheDocument();
    void inputMethodContextTracksCaretNavigation();
    void backspaceMergesParagraphsAndDeletesText();
    void shiftEnterInsertsSoftBreakWithoutSplittingParagraph();
    void shiftNavigationProjectsSelectionRects();
    void dragSelectionCommitsSameRustSelectionProjection();
    void reverseDragMovesInputMethodCaretToFocus();
    void clipboardCopyPasteUsesRustSelectionProjection();
    void selectionDragFromExistingSelectionPublishesCanvasRects();
    void mouseClickClearsSelectionInsideAndOutsideText();
    void multilinePasteSplitsParagraphs();
    void insertReplacementClearsSelectionAndPublishesInteractionChange();
    void crossPageDragPreviewIncludesIntermediatePages();
    void mathTextDragUsesSharedPreviewAndCommitProtocol();
    void documentInputDeltaKeepsUntouchedPagesAndParagraphs();
    void documentFileFlowPreservesSaveStateAndReopens();
    void autoSaveWritesTheCurrentRevisionAfterTheDeadline();
    void saveConflictCannotReplaceAPreviousDocumentAfterSaveAs();
    void recoveryEntriesCanBeRestoredExportedAndDiscarded();
    void documentHistoryUndoRedoTracksRustSession();
    void invalidDocumentFileStatusPreservesThePublishedProjection();
    void interactionResponseErrorsRefreshAutoSaveDeadline();
    void glyphFrameAdapterRejectsFramesOutsideCanvasLimits();
    void underlinedTextRendersTheRustFfiDecorationInTheQtCanvas();
    void documentCaretNavigationUpdatesObservablePosition();
    void confirmedConflictReplacementWritesCurrentDocument();
    void invalidLayoutSnapshotsAreRejectedWithoutReplacingPublishedPages();
    void mathFontFallbackDiagnosticProjectsToViewModel();
    void textFontWarningStatusBarTracksLayoutDiagnostics();
    void missingRecoveryEntryFailsWithoutReplacingTheOpenDocument();
#endif

  private:
#ifdef CARET_ENABLE_APP
    // 文档 runtime 相关用例共用的空白会话与恢复目录设置。
    static bool configureDocumentRecovery(const QString& recovery_root);
    static bool installBlankDocumentRuntime();
#endif
};
