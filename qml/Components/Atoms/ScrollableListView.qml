// 可复用的裁剪滚动列表，统一列表滚动与当前项定位行为。
pragma ComponentBehavior: Bound

import QtQuick

Item {
    id: root

    property var model
    property Component delegate
    property int currentIndex: -1
    property real spacing: 0
    property bool interactive: true
    property int boundsBehavior: Flickable.StopAtBounds

    readonly property alias count: listView.count

    function positionViewAtIndex(index, mode) {
        listView.positionViewAtIndex(index, mode === undefined ? ListView.Contain : mode);
    }

    ListView {
        id: listView
        anchors.fill: parent
        clip: true
        model: root.model
        delegate: root.delegate
        currentIndex: root.currentIndex
        spacing: root.spacing
        interactive: root.interactive
        boundsBehavior: root.boundsBehavior
        onCurrentIndexChanged: {
            if (currentIndex >= 0)
                positionViewAtIndex(currentIndex, ListView.Contain);
        }
    }
}
