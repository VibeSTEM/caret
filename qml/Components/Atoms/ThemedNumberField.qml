// 数值输入原子；浮点格式与输入状态统一，业务范围由调用方指定。
import QtQuick

ThemedTextField {
    implicitHeight: Theme.formControlHeight
    selectByMouse: true
    invalid: text.length > 0 && !acceptableInput
    validator: DoubleValidator {
        locale: "C"
        notation: DoubleValidator.StandardNotation
    }
}
