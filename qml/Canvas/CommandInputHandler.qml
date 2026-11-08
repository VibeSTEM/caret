// 将画布普通按键和命令模式按键映射到 Rust 命令会话。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.ViewModels

Item {
    id: root

    required property var canvas

    function handleTextInput(text) {
        const result = LayoutViewModel.insertText(text);
        if (!result.valid)
            console.warn("Document text input failed:", result.error);
    }

    Connections {
        target: root.canvas
        function onTextInputRequested(text) {
            root.handleTextInput(text);
        }
    }
}
