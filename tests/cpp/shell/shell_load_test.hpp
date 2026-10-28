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
    void unknownIconRefusesColoring();
    void documentModeSelectorTracksLocalChoice();
    void newDocumentDialogHasNoProcessFields();
    void unsavedChangesDialogUsesStyledActionsInRequestedOrder();
    void closeFailureDoesNotReopenPendingConfirmation();
    void documentCanvasMapsPageCoordinatesAndHitsPages();
    void documentUnitsConvertThroughIntegerCentipixels();
    void settingsDialogDraftCancelAndFailure();
#ifdef CARET_ENABLE_APP
    void settingsDialogCommitsPreferencesThroughRust();
#endif
#ifdef CARET_ENABLE_APP
    void documentHitTestReturnsRustPosition();
    void documentInputDeltaKeepsUntouchedPagesAndParagraphs();
    void documentFileFlowPreservesSaveStateAndReopens();
    void autoSaveWritesTheCurrentRevisionAfterTheDeadline();
    void saveConflictCannotReplaceAPreviousDocumentAfterSaveAs();
    void documentHistoryUndoRedoTracksRustSession();
#endif
};
