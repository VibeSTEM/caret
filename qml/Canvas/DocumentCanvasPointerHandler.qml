// 将页面点击转换为 Rust 文档中的插入点。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas
import Caret.ViewModels

Item {
    id: inputHandler

    required property DocumentCanvas canvas

    signal documentPositionHit(var result)

    TapHandler {
        onTapped: function (eventPoint) {
            inputHandler.canvas.forceActiveFocus(Qt.MouseFocusReason);
            const pageHit = inputHandler.canvas.hitTestPage(eventPoint.position);
            if (pageHit.pageIndex < 0)
                return;
            const result = LayoutViewModel.placeCaretAtPage(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
            if (result.valid)
                inputHandler.documentPositionHit(result);
        }
    }
}
