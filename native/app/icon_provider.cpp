#include "icon_provider.hpp"

#include <QColor>
#include <QImage>
#include <QPainter>
#include <QQmlEngine>
#include <QQuickImageProvider>
#include <QRectF>
#include <QRegularExpression>
#include <QSet>
#include <QSize>
#include <QSizeF>
#include <QString>
#include <QSvgRenderer>
#include <QtCore/qminmax.h>
#include <qnamespace.h>

namespace caret {
namespace {

// SVG 的 currentColor 不继承 QML 的颜色；先渲染 alpha，再用调用方颜色着色。
// 每次请求的 renderer/painter 均为局部对象，不共享可变绘图状态。
class IconProvider final : public QQuickImageProvider {
  public:
    IconProvider() : QQuickImageProvider(QQuickImageProvider::Image) {}

    QImage requestImage(const QString& id, QSize* size, const QSize& requested_size) override {
        static const QRegularExpression valid_id(
            QStringLiteral("^([a-z][a-z0-9-]*)/([0-9a-fA-F]{8})$"));
        const auto match = valid_id.match(id);
        if (!match.hasMatch() || !monochrome_icon_names().contains(match.captured(1))) {
            return {};
        }
        QSvgRenderer renderer(
            QStringLiteral(":/qt/qml/Caret/Shell/icons/%1.svg").arg(match.captured(1)));
        if (!renderer.isValid()) {
            return {};
        }
        if (size != nullptr) {
            *size = renderer.defaultSize();
        }
        const QSize pixels = requested_size.isValid() && !requested_size.isEmpty()
                                 ? requested_size
                                 : renderer.defaultSize();
        const QSizeF intrinsic = renderer.defaultSize();
        if (intrinsic.isEmpty()) {
            return {};
        }
        QImage image(pixels, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QPainter painter(&image);
        const qreal scale =
            qMin(pixels.width() / intrinsic.width(), pixels.height() / intrinsic.height());
        const QSizeF rendered(intrinsic.width() * scale, intrinsic.height() * scale);
        const QRectF bounds((pixels.width() - rendered.width()) / 2.0,
                            (pixels.height() - rendered.height()) / 2.0, rendered.width(),
                            rendered.height());
        renderer.render(&painter, bounds);
        painter.setCompositionMode(QPainter::CompositionMode_SourceIn);
        painter.fillRect(image.rect(), QColor(QStringLiteral("#") + match.captured(2)));
        return image;
    }
};

} // namespace

const QSet<QString>& monochrome_icon_names() {
    // 窗口和编辑动作的单色 glyph。彩色资源走 qrc，未知 ID 拒绝着色。
    static const QSet<QString> names{
        QStringLiteral("undo"),     QStringLiteral("redo"),     QStringLiteral("minimize"),
        QStringLiteral("maximize"), QStringLiteral("close"),    QStringLiteral("check"),
        QStringLiteral("copy"),     QStringLiteral("delete"),   QStringLiteral("caret"),
        QStringLiteral("search"),   QStringLiteral("new"),      QStringLiteral("open"),
        QStringLiteral("save"),     QStringLiteral("settings"), QStringLiteral("right"),
    };
    return names;
}

void install_icon_provider(QQmlEngine& engine) {
    engine.addImageProvider(QStringLiteral("caret-icons"), new IconProvider);
}

} // namespace caret
