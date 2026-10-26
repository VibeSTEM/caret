#pragma once

#include <QObject>
#include <QUrl>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// 为保存对话框准备平台文档目录中的默认目标路径。
class DocumentPathAdapter final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

  public:
    explicit DocumentPathAdapter(QObject* parent = nullptr);

    /// 创建 Documents/caret 并返回 Untitled.cdoc 的 file URL；目录不可用时返回空 URL。
    Q_INVOKABLE QUrl defaultSaveFileUrl() const;
};

} // namespace caret
