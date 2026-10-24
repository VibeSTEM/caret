// 公共标题与内容布局；内容显式注入，关闭请求交给窗口守卫。
import QtQuick
import QtQuick.Layouts

Rectangle {
    id: frame
    required property Window window
    property bool closeEnabled: true
    default property alias content: body.data
    anchors.fill: parent
    color: Theme.colorPanel
    ColumnLayout {
        id: body
        anchors.fill: parent
        spacing: 0
        DialogTitleBar {
            Layout.fillWidth: true
            window: frame.window
            caption: frame.window.title
            closeEnabled: frame.closeEnabled
            onCloseRequested: frame.window.close()
        }
    }
}
