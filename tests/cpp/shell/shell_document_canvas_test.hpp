// ShellDocumentCanvasTest 负责对应 Shell 测试领域的可观察行为。
#pragma once

#include "shell_test_support.hpp"

class ShellDocumentCanvasTest final : public ShellTestBase {
    Q_OBJECT

  private slots:
    void preeditCancellationDoesNotReenterTheInputMethodHandler();
    void selectionAndPreviewKeepDifferentPageBoundsPolicies();
    void mathSourceDisablesInputMethodAndCancelsPreedit();
    void canvasPolicyPublishesDocumentedZoomLimits();
    void documentCanvasMapsPageCoordinatesAndHitsPages();
#ifdef CARET_ENABLE_APP
    void glyphFrameAdapterRejectsFramesOutsideCanvasLimits();
    void underlinedTextRendersTheRustFfiDecorationInTheQtCanvas();
    void tableGuidelinesReplacePreviousCellInsteadOfAccumulating();
    void tableBorderLinesKeepUniformThicknessAtFractionalCoordinates();
#endif
};
