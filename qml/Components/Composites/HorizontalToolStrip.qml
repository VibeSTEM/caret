// 标题、菜单与 Ribbon 共用滚动边界；布局完成后保证键盘焦点仍在可见区域。
import QtQuick
import QtQuick.Controls
import QtQml

Flickable {
    id: strip

    // 指向条带内承载工具的内容项；滚动尺寸和焦点坐标使用同一参照。
    required property Item contentRoot

    contentWidth: contentRoot.width
    contentHeight: height
    flickableDirection: Flickable.HorizontalFlick
    boundsBehavior: Flickable.StopAtBounds
    clip: true

    ScrollBar.horizontal: ScrollBar {
        policy: ScrollBar.AsNeeded
    }

    function ensureFocusedVisible() {
        contentX = Math.max(0, Math.min(contentX, contentWidth - width));
        const hostWindow = strip.Window.window;
        const focused = hostWindow ? hostWindow.activeFocusItem : null;
        let ancestor = focused;
        while (ancestor && ancestor !== contentRoot)
            ancestor = ancestor.parent;
        if (!ancestor || !focused)
            return;
        const point = focused.mapToItem(contentRoot, 0, 0);
        const desired = point.x < contentX ? point.x : Math.max(contentX, point.x + focused.width - width);
        contentX = Math.max(0, Math.min(desired, contentWidth - width));
    }

    function scheduleEnsureFocusedVisible() {
        // 嵌套布局会在宽度绑定之后更新子项位置，第二轮再读取最终几何。
        Qt.callLater(function () {
            Qt.callLater(ensureFocusedVisible);
        });
    }
    onWidthChanged: scheduleEnsureFocusedVisible()
    onContentWidthChanged: scheduleEnsureFocusedVisible()
    // contentWidth 只反映内容根宽度绑定；嵌套布局把子项移到最终位置发生在
    // 之后的 polish，childrenRect 变化时必须再滚一次，否则会用旧几何计算
    // 滚动位置且不再重触发（聚焦项停留在视口外）。
    Connections {
        target: strip.contentRoot
        function onChildrenRectChanged() {
            strip.scheduleEnsureFocusedVisible();
        }
    }
    Connections {
        target: strip.Window.window
        function onActiveFocusItemChanged() {
            strip.scheduleEnsureFocusedVisible();
        }
    }
}
