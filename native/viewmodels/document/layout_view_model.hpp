/// 把 Rust 生成的只读页面快照转换为 QML 可绑定的值。
#pragma once

#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QString>
#include <QThreadPool>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
#include <memory>

namespace caret {

namespace layout_snapshot {
struct ParsedLayoutSnapshot;
}

struct DocumentOpenTicketOwner;

struct DocumentOpenTaskResult final {
    qulonglong ticket = 0;
    std::shared_ptr<DocumentOpenTicketOwner> ticket_owner;
    std::shared_ptr<layout_snapshot::ParsedLayoutSnapshot> snapshot;
    QString error;
};

/// 在创建 QML 引擎前安装 Rust 返回的页面快照 JSON。
bool install_layout_snapshot(const QString& snapshot_json);

class LayoutViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QVariantList pageSizesPt READ pageSizesPt NOTIFY layoutChanged)
    Q_PROPERTY(QVariantList pageGlyphRevisions READ pageGlyphRevisions NOTIFY layoutChanged)
    Q_PROPERTY(QVariantList pageLayouts READ pageLayouts NOTIFY layoutChanged)
    Q_PROPERTY(QVariantMap pageMarginsPt READ pageMarginsPt NOTIFY layoutChanged)
    Q_PROPERTY(QVariantList selectionRects READ selectionRects NOTIFY interactionChanged)
    Q_PROPERTY(QVariantList compositionRects READ compositionRects NOTIFY layoutChanged)
    Q_PROPERTY(QVariantList paragraphLayouts READ paragraphLayouts NOTIFY layoutChanged)
    Q_PROPERTY(QVariantMap inputMethodContext READ inputMethodContext NOTIFY interactionChanged)
    Q_PROPERTY(QString documentGeneration READ documentGeneration NOTIFY layoutChanged)
    Q_PROPERTY(QString fontGeneration READ fontGeneration NOTIFY layoutChanged)
    Q_PROPERTY(qulonglong layoutRevision READ layoutRevision NOTIFY layoutChanged)
    Q_PROPERTY(QString error READ error CONSTANT)
    Q_PROPERTY(QString documentPath READ documentPath NOTIFY documentFileChanged)
    Q_PROPERTY(QString documentFileName READ documentFileName NOTIFY documentFileChanged)
    Q_PROPERTY(bool documentEdited READ documentEdited NOTIFY documentFileChanged)
    Q_PROPERTY(bool documentFileBusy READ documentFileBusy NOTIFY documentFileChanged)
    Q_PROPERTY(bool documentOpening READ documentOpening NOTIFY documentFileChanged)
    Q_PROPERTY(QString documentSaveStatus READ documentSaveStatus NOTIFY documentFileChanged)
    Q_PROPERTY(QVariantMap caretGeometry READ caretGeometry NOTIFY caretGeometryChanged)

  public:
    explicit LayoutViewModel(QObject* parent = nullptr);

    [[nodiscard]] const QVariantList& pageSizesPt() const { return m_page_sizes_pt; }
    [[nodiscard]] const QVariantList& pageGlyphRevisions() const { return m_page_glyph_revisions; }
    [[nodiscard]] QVariantList pageLayouts() const { return m_page_layouts.toVariantList(); }
    [[nodiscard]] const QVariantMap& pageMarginsPt() const { return m_page_margins_pt; }
    [[nodiscard]] const QVariantList& selectionRects() const { return m_selection_rects; }
    [[nodiscard]] const QVariantList& compositionRects() const { return m_composition_rects; }
    [[nodiscard]] QVariantList paragraphLayouts() const {
        return m_paragraph_layouts.toVariantList();
    }
    [[nodiscard]] const QVariantMap& inputMethodContext() const { return m_input_method_context; }
    [[nodiscard]] QString documentGeneration() const {
        return QString::number(m_document_generation);
    }
    [[nodiscard]] QString fontGeneration() const { return QString::number(m_font_generation); }
    [[nodiscard]] qulonglong layoutRevision() const { return m_layout_revision; }
    [[nodiscard]] const QString& error() const { return m_error; }
    [[nodiscard]] const QString& documentPath() const { return m_document_path; }
    [[nodiscard]] const QString& documentFileName() const { return m_document_file_name; }
    [[nodiscard]] bool documentEdited() const { return m_document_edited; }
    [[nodiscard]] bool documentFileBusy() const { return m_document_file_busy; }
    [[nodiscard]] bool documentOpening() const { return m_document_opening; }
    [[nodiscard]] QString documentSaveStatus() const {
        if (m_pending_save_operations != 0) {
            return QStringLiteral("saving");
        }
        if (m_document_edited) {
            return QStringLiteral("edited");
        }
        return m_document_path.isEmpty() ? QStringLiteral("clean") : QStringLiteral("saved");
    }
    [[nodiscard]] const QVariantMap& caretGeometry() const { return m_caret_geometry; }

    /// 将页面局部 pt 坐标交给 Rust 布局快照，返回带文档位置的命中结果。
    Q_INVOKABLE QVariantMap hitTestDocument(int page_index, qreal x_pt, qreal y_pt);
    /// 在 Rust 会话中将页面命中位置设为当前插入点。
    Q_INVOKABLE QVariantMap placeCaretAtPage(int page_index, qreal x_pt, qreal y_pt);
    /// 将 Qt 键盘输入文本提交给 Rust 会话并安装新布局快照。
    Q_INVOKABLE QVariantMap insertText(const QString& text);
    /// 将 Return 提交给 Rust，并安装拆分后的页面快照。
    Q_INVOKABLE QVariantMap splitParagraph();
    /// 将 Backspace 提交给 Rust，并安装删除后的页面快照。
    Q_INVOKABLE QVariantMap deleteBackward();
    /// 将 Shift+Return 提交为段内软换行。
    Q_INVOKABLE QVariantMap insertSoftBreak();
    /// 按视觉顺序移动 caret，并由 Rust 更新布局快照。
    Q_INVOKABLE QVariantMap moveVisualCaret(bool forward, bool extend);
    /// 在上下视觉行移动 caret。
    Q_INVOKABLE QVariantMap moveVerticalCaret(int line_delta, bool extend);
    /// 移到当前视觉行首尾。
    Q_INVOKABLE QVariantMap moveToVisualLineEdge(bool end, bool extend);
    /// 将 Qt 输入法预编辑、提交及替换范围交给 Rust Session。
    Q_INVOKABLE QVariantMap inputMethodEvent(const QString& preedit, const QString& commit,
                                             int replacement_start, int replacement_length,
                                             int cursor_position_utf16);
    /// 按需提供当前页拖选缓存；指针移动过程只在 Qt/QML 内部命中。
    Q_INVOKABLE QVariantMap dragHitMap(int page_index);
    /// 鼠标释放时一次性提交本地预览锚点与焦点。
    Q_INVOKABLE QVariantMap setSelection(const QVariantMap& anchor, const QVariantMap& focus);
    /// 清除选区并保留现有焦点位置。
    Q_INVOKABLE QVariantMap clearSelection();
    /// 将 Rust 生成的选区纯文本复制到系统剪贴板。
    Q_INVOKABLE QVariantMap copySelectionToClipboard();
    /// 将系统剪贴板文本作为一次 Rust 插入/替换命令粘贴。
    Q_INVOKABLE QVariantMap pasteFromClipboard();
    /// 在后台读取 `.cdoc`，再由 Rust 在 GUI 线程验证并安装新会话。
    Q_INVOKABLE QVariantMap openDocument(const QString& path);
    /// 捕获当前修订后在后台原子保存；空路径表示保存到当前文档路径。
    Q_INVOKABLE QVariantMap saveDocument(const QString& path = QString{});
    /// 只请求当前可见页的 glyph 栅格；图像像素以 QByteArray 二进制返回。
    Q_INVOKABLE QVariantMap rasterizePageGlyphs(int page_index, qreal px_per_pt,
                                                qreal page_origin_device_x_px,
                                                qreal page_origin_device_y_px, int foreground_red,
                                                int foreground_green, int foreground_blue,
                                                int foreground_alpha);

  private:
    bool installSnapshot(const QString& snapshot_json, bool replace_document = false);
    bool installSnapshot(const QJsonDocument& snapshot_document, bool replace_document = false);
    bool installSnapshot(layout_snapshot::ParsedLayoutSnapshot parsed_snapshot);
    QVariantMap installEditResponse(const QByteArray& response_bytes);
    QVariantMap installEditResponse(const QJsonDocument& snapshot_document);
    QVariantMap installInteractionResponse(const QByteArray& response_bytes);
    bool installInteraction(const QString& interaction_json);
    bool installInteraction(const QJsonObject& interaction_object);
    void installCaretGeometry(const QVariantMap& geometry);
    void refreshDocumentFileStatus();
    void setDocumentFileBusy(bool busy);

  signals:
    void layoutChanged();
    void interactionChanged();
    void caretGeometryChanged();
    void documentFileChanged();
    void documentFileOperationFinished(bool succeeded, const QString& error);

  private:
    QVariantList m_page_sizes_pt;
    QVariantList m_page_glyph_revisions;
    QJsonArray m_page_layouts;
    QVariantMap m_page_margins_pt;
    QVariantList m_selection_rects;
    QVariantList m_composition_rects;
    QJsonArray m_paragraph_layouts;
    QVariantMap m_input_method_context;
    qulonglong m_document_generation = 0;
    qulonglong m_font_generation = 0;
    qulonglong m_layout_revision = 0;
    QString m_error;
    QVariantMap m_caret_geometry;
    QString m_document_path;
    QString m_document_file_name = QStringLiteral("Untitled");
    bool m_document_edited = false;
    bool m_document_file_busy = false;
    bool m_document_opening = false;
    int m_pending_file_operations = 0;
    int m_pending_save_operations = 0;
    QString m_pending_save_target;
    QThreadPool m_document_io_pool;
};

} // namespace caret
