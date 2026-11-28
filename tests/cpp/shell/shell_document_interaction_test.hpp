// ShellDocumentInteractionTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellDocumentInteractionTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
#ifdef CARET_ENABLE_APP
    void mathFontFallbackDiagnosticProjectsToViewModel();
    void textFontWarningStatusBarTracksLayoutDiagnostics();
    void environmentHighlightSnapshotSelectsAndClearsActiveRects();
    void commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument();
    void inputMethodContextFlowsFromViewModelToCanvas();
    void cancelledInputMethodStateAllowsTheNextCommit();
    void documentCanvasRoutesCommandKeysAndKeepsNormalEditingBehavior();
    void documentHitTestReturnsRustPosition();
    void documentCanvasClickAndTypingPublishRustSnapshot();
    void tableGuidelinesRefreshForArrowAndCellClick();
    void tableThemeTokensRenderAcrossZoomAndWindowFocus();
    void tablePaintUsesBuiltinThemeColors();
    void tableBoundaryCaretAndShiftArrowsUseWholeGrid();
    void tableGuidelineUsesTheVisiblePageFragment();
    void tableDragSelectsAcrossUnevenCells();
    void tableDragFromPreeditSelectsAcrossCells();
    void textDragFromPreeditKeepsTheMappedAnchor();
    void tableDragSelectsACellContainingOnlyDisplayMath();
    void tableShapeShortcutsMutateThroughTheFocusedViewModel();
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
    void richClipboardCopyPreservesMathStructure();
    void richClipboardPasteRoundTripsMathStructureAndUndo();
    void richClipboardBlocksPasteRoundTripsDisplayMathAndUndo();
    void richClipboardRoundTripsATableAndItsDisplayMath();
    void richClipboardRejectsInvalidFragmentPayload();
    void externalMathTextPasteRecognizesDelimiters();
    void selectionDragFromExistingSelectionPublishesCanvasRects();
    void mouseClickClearsSelectionInsideAndOutsideText();
    void multilinePasteSplitsParagraphs();
    void insertReplacementClearsSelectionAndPublishesInteractionChange();
    void crossPageDragPreviewIncludesIntermediatePages();
    void mathTextDragUsesSharedPreviewAndCommitProtocol();
    void emptyInlineMathFocusRendersInsertionSlotAcrossZoom();
    void documentInputDeltaKeepsUntouchedPagesAndParagraphs();
    void invalidDocumentFileStatusPreservesThePublishedProjection();
    void interactionResponseErrorsRefreshAutoSaveDeadline();
#endif
};
