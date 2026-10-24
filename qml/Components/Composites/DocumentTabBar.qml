pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Shapes

Item {
    id: bar

    implicitWidth: Theme.documentTabWidth + 2 * Theme.spacingTiny
    implicitHeight: Theme.documentTabBarHeight

    // ViewModel 投影：[{id, kind, state, title, message}]；顺序即页签顺序。
    property var documents: []
    property string activeDocumentId: ""
    // 由宿主传入；true 时跳过页签位移动画。
    property bool reducedMotion: false

    signal activateDocument(string documentId)
    signal closeDocument(string documentId)
    signal moveDocument(int fromIndex, int toIndex)

    Accessible.role: Accessible.PageTabList
    Accessible.name: qsTr("Open documents")

    clip: true

    // 委托身份在重排时保持稳定；源模型换序后只更新条目内容和槽位。
    property var displayDocuments: []
    // 拖拽时先预览顺序，松手再向 ViewModel 提交。
    property var order: []
    property string draggedId: ""
    // 拖拽中页签的 x（Flickable 内容坐标）。
    property real dragX: 0
    readonly property bool dragging: draggedId !== ""
    onDocumentsChanged: {
        internal.pressedId = "";
        if (dragging) {
            draggedId = "";
        }
        sync_documents();
    }

    function sync_documents() {
        const ids = [];
        const byId = new Map();
        for (const document of documents) {
            ids.push(document.id);
            byId.set(document.id, document);
        }
        const sameIds = displayDocuments.length === ids.length && displayDocuments.every(entry => byId.has(entry.id));
        displayDocuments = sameIds ? displayDocuments.map(entry => byId.get(entry.id)) : Array.from(documents);
        order = ids;
    }
    function slot_x(index) {
        return Theme.spacingTiny + index * (Theme.documentTabWidth + Theme.spacingTiny);
    }
    function slot_of(id) {
        return order.indexOf(id);
    }
    function document_title(document) {
        return document.kind === "welcome" ? qsTranslate("ProjectViewModel", "Welcome") : document.title ?? "";
    }
    function display_title(document) {
        const title = document_title(document);
        if (document.kind !== "import" || (document.displayCount ?? 1) < 2) {
            return title;
        }
        return `${document.displayOrdinal} · ${title}`;
    }
    function accessible_title(document) {
        if (document.kind !== "import" || (document.displayCount ?? 1) < 2) {
            return document_title(document);
        }
        return qsTr("%1, import %2").arg(document_title(document)).arg(document.displayOrdinal);
    }
    function accessible_description(document) {
        if (document.state === "loading") {
            return qsTr("Loading");
        }
        if (document.state === "failed") {
            return qsTranslate("DocumentTabBar", document.message ?? qsTr("Failed to load"));
        }
        if (document.state === "unloaded") {
            return qsTr("Mesh data released; activate to reload");
        }
        return "";
    }
    function focus_document(index) {
        const tab = tabRepeater.itemAt(index);
        const document = displayDocuments[index];
        if (!tab || !document) {
            return;
        }
        const left = tab.x;
        const right = left + tab.width;
        if (left < scroller.contentX) {
            scroller.contentX = left;
        } else if (right > scroller.contentX + scroller.width) {
            scroller.contentX = right - scroller.width;
        }
        tab.forceActiveFocus();
        if (document.state === "ready" || document.state === "unloaded") {
            bar.activateDocument(document.id);
        }
    }

    Component.onCompleted: sync_documents()

    Rectangle {
        anchors.fill: parent
        color: Theme.colorDocumentBand
    }
    Rectangle {
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: Theme.borderWidth
        color: Theme.colorDocumentBandLine
    }
    // 带下缘连续白线：活动页签的圆弧件覆盖其两侧，交界即圆弧过渡。
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Theme.documentTabBottomLineHeight
        color: Theme.colorPanel
    }

    Flickable {
        id: scroller
        objectName: "documentTabScroller"
        anchors.fill: parent
        contentWidth: 2 * Theme.spacingTiny + bar.documents.length * (Theme.documentTabWidth + Theme.spacingTiny) + Theme.documentTabRadius - 2 * Theme.spacingTiny
        clip: false
        interactive: contentWidth > width && !bar.dragging
        boundsBehavior: Flickable.StopAtBounds

        Repeater {
            id: tabRepeater
            model: bar.displayDocuments.length

            Item {
                id: tab
                required property int index
                objectName: "documentTab-" + documentId

                readonly property var doc: bar.displayDocuments[index] ?? {}
                readonly property string documentId: doc.id ?? ""
                readonly property string tabState: doc.state ?? "ready"
                readonly property bool isActive: documentId === bar.activeDocumentId
                readonly property bool isDragged: documentId === bar.draggedId
                Accessible.role: Accessible.PageTab
                Accessible.name: bar.accessible_title(tab.doc)
                Accessible.description: bar.accessible_description(tab.doc)
                Accessible.selected: tab.isActive
                Accessible.onPressAction: {
                    if (tab.tabState === "ready" || tab.tabState === "unloaded") {
                        tab.forceActiveFocus();
                        bar.activateDocument(tab.documentId);
                    }
                }
                activeFocusOnTab: true
                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Left || event.key === Qt.Key_Right) {
                        const count = bar.displayDocuments.length;
                        if (count > 0) {
                            const direction = event.key === Qt.Key_Right ? 1 : -1;
                            bar.focus_document((tab.index + direction + count) % count);
                        }
                        event.accepted = true;
                    } else if ((event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && (tab.tabState === "ready" || tab.tabState === "unloaded")) {
                        bar.activateDocument(tab.documentId);
                        event.accepted = true;
                    }
                }
                Behavior on x {
                    enabled: !tab.isDragged && !bar.reducedMotion
                    NumberAnimation {
                        id: slideAnimation
                        duration: Theme.documentTabAnimationDuration
                        easing.type: Easing.OutQuad
                    }
                }
                // HTML 标签带的 1px 上边框 + 2px 上内边距。
                y: Theme.borderWidth + Theme.spacingTiny
                x: isDragged ? bar.dragX : bar.slot_x(bar.slot_of(documentId))

                width: Theme.documentTabWidth
                height: Theme.documentTabBarHeight - y - Theme.documentTabBottomLineHeight
                z: tab.isDragged || slideAnimation.running ? 2 : tab.isActive ? 1 : 0

                Rectangle {
                    anchors.fill: parent
                    topLeftRadius: Theme.documentTabRadius
                    topRightRadius: Theme.documentTabRadius
                    color: tab.isActive ? Theme.colorPanel : tabArea.containsMouse ? Theme.colorDocumentHover : Theme.colorTransparent
                    border.width: tab.activeFocus ? Theme.focusBorderWidth : 0
                    border.color: Theme.colorTransparent
                }
                // HTML 伪元素的 8px 圆角阴影只在圆弧外露出面板色；
                // PathArc 保持真圆；与主体重叠 1px 避免抗锯齿接缝。
                Shape {
                    visible: tab.isActive
                    x: -Theme.documentTabRadius
                    y: tab.height - Theme.documentTabRadius
                    width: Theme.documentTabRadius + Theme.borderWidth
                    height: Theme.documentTabRadius
                    z: 2
                    ShapePath {
                        fillColor: Theme.colorPanel
                        strokeColor: Theme.colorTransparent
                        strokeWidth: 0
                        startX: Theme.documentTabRadius
                        startY: 0
                        PathArc {
                            x: 0
                            y: Theme.documentTabRadius
                            radiusX: Theme.documentTabRadius
                            radiusY: Theme.documentTabRadius
                        }
                        PathLine {
                            x: Theme.documentTabRadius + Theme.borderWidth
                            y: Theme.documentTabRadius
                        }
                        PathLine {
                            x: Theme.documentTabRadius + Theme.borderWidth
                            y: 0
                        }
                    }
                }
                Shape {
                    visible: tab.isActive
                    x: tab.width - Theme.borderWidth
                    y: tab.height - Theme.documentTabRadius
                    width: Theme.documentTabRadius + Theme.borderWidth
                    height: Theme.documentTabRadius
                    z: 2
                    ShapePath {
                        fillColor: Theme.colorPanel
                        strokeColor: Theme.colorTransparent
                        strokeWidth: 0
                        startX: Theme.borderWidth
                        startY: 0
                        PathArc {
                            x: Theme.documentTabRadius + Theme.borderWidth
                            y: Theme.documentTabRadius
                            radiusX: Theme.documentTabRadius
                            radiusY: Theme.documentTabRadius
                            direction: PathArc.Counterclockwise
                        }
                        PathLine {
                            x: Theme.borderWidth
                            y: Theme.documentTabRadius
                        }
                        PathLine {
                            x: Theme.borderWidth
                            y: 0
                        }
                    }
                }

                MouseArea {
                    id: tabArea
                    objectName: "documentTabDragArea"
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: pressed && tab.isDragged ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                    acceptedButtons: Qt.LeftButton
                    // Flickable 抢抓会中断长距离拖动；空白区仍可滚动。
                    preventStealing: true
                    onPressed: mouse => {
                        if (mouse.button !== Qt.LeftButton) {
                            return;
                        }
                        tab.forceActiveFocus(Qt.MouseFocusReason);
                        const p = scroller.contentItem.mapFromItem(tab, mouse.x, mouse.y);
                        bar.begin_press(tab.documentId, p.x, p.y);
                    }
                    onPositionChanged: mouse => {
                        if (!(mouse.buttons & Qt.LeftButton)) {
                            return;
                        }
                        // 滚动后指针和槽位必须使用同一内容坐标系。
                        const p = scroller.contentItem.mapFromItem(tab, mouse.x, mouse.y);
                        if (!bar.dragging) {
                            bar.detect_drag(tab.documentId, p.x, p.y);
                        } else {
                            bar.drag_move(p.x);
                        }
                    }
                    onReleased: mouse => {
                        if (bar.dragging) {
                            const p = scroller.contentItem.mapFromItem(tab, mouse.x, mouse.y);
                            bar.drag_move(p.x);
                        }
                        bar.finish_drag(true);
                    }
                    onCanceled: bar.finish_drag(false)
                    onClicked: mouse => {
                        if (!internal.suppressClick) {
                            bar.activateDocument(tab.documentId);
                        }
                    }
                }

                Item {
                    id: labelClip
                    anchors.left: parent.left
                    anchors.leftMargin: Theme.documentTabContentInset
                    anchors.right: closeButton.left
                    anchors.rightMargin: Theme.spacingXSmall
                    anchors.verticalCenter: parent.verticalCenter
                    height: parent.height
                    clip: true

                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: Theme.spacingMedium

                        Rectangle {
                            visible: (tab.doc.kind ?? "") === "welcome"
                            width: Theme.documentTabWelcomeIconSize
                            height: Theme.documentTabWelcomeIconSize
                            radius: Theme.radiusSmall
                            color: Theme.colorBrand
                            Accessible.ignored: true
                        }
                        BusyIndicator {
                            visible: tab.tabState === "loading"
                            width: Theme.iconSizeSmall
                            height: Theme.iconSizeSmall
                            running: visible
                        }
                        ThemedIcon {
                            visible: (tab.doc.kind ?? "") !== "welcome" && tab.tabState !== "loading" && (tab.doc.icon ?? "") !== ""
                            name: tab.doc.icon ?? ""
                            iconSize: Theme.iconSizeSmall
                            preserveSourceColors: tab.doc.preserveIconColors ?? false
                        }
                        ThemedLabel {
                            objectName: "documentTabTitle"
                            width: Math.min(implicitWidth, Math.max(0, labelClip.width - (tab.doc.kind === "welcome" ? Theme.documentTabWelcomeIconSize : Theme.iconSizeSmall) - Theme.spacingMedium))
                            text: bar.display_title(tab.doc)
                            textSize: Theme.fontBody
                            color: tab.tabState === "failed" ? Theme.colorError : tab.isActive ? Theme.colorText : Theme.colorTextMuted
                            elide: Text.ElideRight
                        }
                    }
                }

                Rectangle {
                    id: closeButton
                    objectName: "documentTabClose"
                    anchors.right: parent.right
                    anchors.rightMargin: Theme.documentTabCloseRightInset
                    anchors.verticalCenter: parent.verticalCenter
                    width: Theme.documentTabCloseSize
                    height: Theme.documentTabCloseSize
                    radius: Theme.documentTabCloseRadius
                    color: closeArea.pressed ? Theme.colorDocumentClosePressed : closeArea.containsMouse ? Theme.colorHover : Theme.colorTransparent
                    border.width: closeButton.activeFocus ? Theme.focusBorderWidth : 0
                    border.color: Theme.colorFocus

                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Close %1").arg(bar.accessible_title(tab.doc))
                    Accessible.onPressAction: bar.closeDocument(tab.documentId)
                    activeFocusOnTab: true
                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            bar.closeDocument(tab.documentId);
                            event.accepted = true;
                        }
                    }

                    ThemedIcon {
                        anchors.centerIn: parent
                        name: "close"
                        iconSize: Theme.iconSizeCompact
                        color: closeArea.containsMouse || closeArea.pressed ? Theme.colorText : tab.isActive ? Theme.colorIcon : Theme.colorTextMuted
                    }
                    MouseArea {
                        id: closeArea
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: bar.closeDocument(tab.documentId)
                    }
                }
            }
        }
    }

    QtObject {
        id: internal
        property real pressX: 0
        property real pressY: 0
        property string pressedId: ""
        property real grabOffset: 0
        property bool suppressClick: false
    }

    function begin_press(documentId, x, y) {
        internal.pressedId = documentId;
        internal.pressX = x;
        internal.pressY = y;
        internal.suppressClick = false;
    }
    function detect_drag(documentId, pointerX, pointerY) {
        if (internal.pressedId !== documentId || bar.dragging) {
            return;
        }
        const dx = pointerX - internal.pressX;
        const dy = pointerY - internal.pressY;
        if (Math.max(Math.abs(dx), Math.abs(dy)) < Theme.documentTabDragThreshold) {
            return;
        }
        if (Math.abs(dx) <= Math.abs(dy)) {
            internal.pressedId = "";
            return;
        }
        // 开始拖拽时保留指针在页签中的位置；否则越过阈值的首帧会跳位。
        internal.suppressClick = true;
        const document = bar.documents.find(entry => entry.id === documentId);
        if (document && (document.state === "ready" || document.state === "unloaded")) {
            bar.activateDocument(documentId);
        }
        let visualX = slot_x(slot_of(documentId));
        for (let i = 0; i < tabRepeater.count; ++i) {
            const item = tabRepeater.itemAt(i);
            if (item && item.documentId === documentId) { // qmllint disable missing-property
                visualX = item.x;
                break;
            }
        }
        internal.grabOffset = internal.pressX - visualX;
        bar.dragX = visualX;
        bar.draggedId = documentId;
        drag_move(pointerX);
    }
    function drag_move(pointerX) {
        if (!bar.dragging) {
            return;
        }
        // 只显示当前可视区内的拖动标签；槽位判定使用未钳制的内容坐标。
        const left = scroller.contentX;
        const right = Math.max(left, left + scroller.width - Theme.documentTabWidth);
        bar.dragX = Math.max(left, Math.min(pointerX - internal.grabOffset, right));
        const draggedIndex = order.indexOf(bar.draggedId);
        if (draggedIndex < 0) {
            return;
        }
        const slotSpan = Theme.documentTabWidth + Theme.spacingTiny;
        const pointerSlot = Math.round((pointerX - Theme.spacingTiny) / slotSpan - 0.5);
        const target = pointerX < slot_x(pointerSlot) + Theme.documentTabWidth / 2 ? pointerSlot : pointerSlot + 1;
        const clamped = Math.max(0, Math.min(order.length - 1, target));
        if (clamped === draggedIndex) {
            return;
        }
        const next = order.slice();
        next.splice(draggedIndex, 1);
        next.splice(clamped, 0, bar.draggedId);
        order = next;
    }
    function finish_drag(commit) {
        internal.pressedId = "";
        if (!bar.dragging) {
            return;
        }
        const from = bar.documents.findIndex(entry => entry.id === bar.draggedId);
        const to = order.indexOf(bar.draggedId);
        bar.draggedId = "";
        if (!commit) {
            sync_documents();
        } else if (from >= 0 && to >= 0 && from !== to) {
            moveDocument(from, to);
        }
    }
}
