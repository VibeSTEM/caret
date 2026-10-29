// 将画布普通按键和命令模式按键映射到 Rust 命令会话。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.ViewModels

Item {
    id: root

    required property var canvas

    function sendCommand(action, text) {
        const result = LayoutViewModel.commandLineCommand(action, text || "");
        if (!result.valid)
            console.warn("Document command input failed:", result.error);
        return result;
    }

    function handleTextInput(text) {
        const commandLine = LayoutViewModel.commandLine;
        if (commandLine.active === true) {
            sendCommand("type", text);
            return;
        }
        if (text === "\\" && root.canvas.preeditText.length === 0) {
            sendCommand("begin", "");
            return;
        }
        const result = LayoutViewModel.insertText(text);
        if (!result.valid)
            console.warn("Document text input failed:", result.error);
    }

    function handleCommandKey(key) {
        if (key === "space") {
            sendCommand("type", " ");
            return;
        }
        sendCommand(key === "tab" ? "tab" : key, "");
    }

    Connections {
        target: root.canvas
        function onTextInputRequested(text) {
            root.handleTextInput(text);
        }
        function onCommandLineKeyRequested(key) {
            root.handleCommandKey(key);
        }
    }
}
