// 装饰性图标：单色符号由宿主着色，彩色符号保留资源配色；动作语义由宿主承担。
import QtQuick

Image {
    property string name: ""
    property int iconSize: Theme.iconSizeDefault
    property color color: Theme.colorIcon
    // 彩色符号直接载入 SVG；utility glyph 才交给 provider 按 Theme 状态着色。
    property bool preserveSourceColors: false

    // 固定八位 ARGB，避免 color 字符串省略不透明 alpha 或 URL 中的 # 片段。
    readonly property string colorKey: {
        if (preserveSourceColors) {
            return "";
        }
        const channels = [color.a, color.r, color.g, color.b];
        return channels.map(channel => Math.round(channel * 255).toString(16).padStart(2, "0")).join("");
    }

    width: iconSize
    height: iconSize
    source: {
        if (name === "") {
            return "";
        }
        if (preserveSourceColors) {
            return "qrc:/qt/qml/Caret/Shell/icons/" + name + ".svg";
        }
        return "image://caret-icons/" + name + "/" + colorKey;
    }
    sourceSize: Qt.size(iconSize, iconSize)
    fillMode: Image.PreserveAspectFit
    Accessible.ignored: true
}
