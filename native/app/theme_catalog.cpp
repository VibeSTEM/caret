#include "theme_catalog.hpp"

#include "caret_ffi.h"
#include "theme_view_model.hpp"
#include <QByteArray>
#include <QColor>
#include <QHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QString>
#include <QVariant>
#include <QVariantMap>
#include <cstdio>
#include <qcontainerfwd.h>
#include <utility>

namespace caret {
namespace {

bool fail(const char* detail) {
    std::fprintf(stderr, "caret: 主题目录不可用：%s\n", detail);
    return false;
}

bool insert_token(const QString& name, const QJsonObject& spec, QVariantMap& tokens) {
    const QString kind = spec.value(QStringLiteral("kind")).toString();
    const QJsonValue value = spec.value(QStringLiteral("value"));
    if (kind == QStringLiteral("color")) {
        if (!value.isString()) {
            return false;
        }
        const QColor color = QColor::fromString(value.toString());
        if (!color.isValid()) {
            return false;
        }
        tokens.insert(name, color);
        return true;
    }
    if (kind == QStringLiteral("int")) {
        if (!value.isDouble()) {
            return false;
        }
        tokens.insert(name, value.toInt());
        return true;
    }
    if (kind == QStringLiteral("real")) {
        if (!value.isDouble()) {
            return false;
        }
        tokens.insert(name, value.toDouble());
        return true;
    }
    if (kind == QStringLiteral("bool")) {
        if (!value.isBool()) {
            return false;
        }
        tokens.insert(name, value.toBool());
        return true;
    }
    return false;
}

} // namespace

bool install_builtin_theme_catalog() {
    const rust::String payload = caret::ffi::builtin_theme_catalog();
    const auto size = static_cast<qsizetype>(payload.size());
    const QByteArray bytes(payload.data(), size);
    if (bytes.startsWith("error:")) {
        std::fprintf(stderr, "caret: 主题目录不可用：%s\n", bytes.constData());
        return false;
    }

    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        return fail(parse_error.error == QJsonParseError::NoError
                        ? "主题目录不是对象"
                        : qPrintable(parse_error.errorString()));
    }

    const QJsonObject root = document.object();
    const QString active = root.value(QStringLiteral("active")).toString();
    const QJsonValue themes_value = root.value(QStringLiteral("themes"));
    if (active.isEmpty() || !themes_value.isObject()) {
        return fail("缺少活动主题");
    }
    const QJsonObject themes = themes_value.toObject();
    if (!themes.contains(active)) {
        return fail("活动主题不在目录中");
    }

    QHash<QString, QVariantMap> snapshots;
    for (auto theme = themes.constBegin(); theme != themes.constEnd(); ++theme) {
        if (!theme.value().isObject()) {
            return fail("主题快照不是对象");
        }
        QVariantMap tokens;
        const QJsonObject entries = theme.value().toObject();
        for (auto token = entries.constBegin(); token != entries.constEnd(); ++token) {
            if (!token.value().isObject() ||
                !insert_token(token.key(), token.value().toObject(), tokens)) {
                return fail("主题 token 无法读取");
            }
        }
        if (tokens.isEmpty()) {
            return fail("主题快照是空的");
        }
        snapshots.insert(theme.key(), tokens);
    }

    install_theme_snapshots(std::move(snapshots), active);
    return true;
}

} // namespace caret
