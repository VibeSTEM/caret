// 提供平台文档目录与保存目标路径适配。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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

    /// 创建 Documents/caret 并返回首个未占用的 Untitled.cdoc 名称；目录不可用时返回空 URL。
    Q_INVOKABLE QUrl defaultSaveFileUrl() const;
    /// 将本地绝对路径转换为文件 URL，供 Save As 对话框定位到当前文档。
    Q_INVOKABLE QUrl localFileUrl(const QString& path) const;
    /// 从文件对话框返回的本地 URL 提取文件系统路径。
    Q_INVOKABLE QString localFilePath(const QUrl& url) const;
};

} // namespace caret
