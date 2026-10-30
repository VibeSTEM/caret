// Shell 唯一主题门面；默认值和主题覆盖经 Rust schema 校验后由 ViewModel 发布。
pragma Singleton

import QtQuick
import Caret.ViewModels

QtObject {
    // 界面色彩
    readonly property color colorPanel: ThemeViewModel.tokens.colorPanel
    readonly property color colorTransparent: ThemeViewModel.tokens.colorTransparent
    readonly property color colorChrome: ThemeViewModel.tokens.colorChrome
    readonly property color colorChromePressed: ThemeViewModel.tokens.colorChromePressed
    readonly property color colorChromeLine: ThemeViewModel.tokens.colorChromeLine
    readonly property color colorPanelLine: ThemeViewModel.tokens.colorPanelLine
    readonly property color colorCaret: ThemeViewModel.tokens.colorCaret
    readonly property color colorStatus: ThemeViewModel.tokens.colorStatus
    readonly property color colorText: ThemeViewModel.tokens.colorText
    readonly property color colorTextMuted: ThemeViewModel.tokens.colorTextMuted
    readonly property color colorIcon: ThemeViewModel.tokens.colorIcon
    readonly property color colorHover: ThemeViewModel.tokens.colorHover
    readonly property color colorSelected: ThemeViewModel.tokens.colorSelected
    readonly property color colorCommandPrompt: ThemeViewModel.tokens.colorCommandPrompt
    readonly property color colorBlockEnvironmentHighlight: ThemeViewModel.tokens.colorBlockEnvironmentHighlight
    readonly property color colorInlineEnvironmentHighlight: ThemeViewModel.tokens.colorInlineEnvironmentHighlight
    readonly property color colorFocus: ThemeViewModel.tokens.colorFocus
    readonly property color colorError: ThemeViewModel.tokens.colorError
    readonly property color colorMenubarText: ThemeViewModel.tokens.colorMenubarText
    readonly property color colorMenubarHover: ThemeViewModel.tokens.colorMenubarHover
    readonly property color colorCloseHover: ThemeViewModel.tokens.colorCloseHover
    readonly property color colorDialogPrimary: ThemeViewModel.tokens.colorDialogPrimary
    readonly property color colorDialogPrimaryBorder: ThemeViewModel.tokens.colorDialogPrimaryBorder

    // 光标几何
    readonly property int caretWidthCentipixels: ThemeViewModel.tokens.caretWidthCentipixels

    // 字体与字号
    readonly property int fontTitle: ThemeViewModel.tokens.fontTitle
    readonly property int fontBody: ThemeViewModel.tokens.fontBody
    readonly property int fontSmall: ThemeViewModel.tokens.fontSmall

    // 通用间距（逻辑像素）
    readonly property int spacingTiny: ThemeViewModel.tokens.spacingTiny
    readonly property int spacingXSmall: ThemeViewModel.tokens.spacingXSmall
    readonly property int spacingSmall: ThemeViewModel.tokens.spacingSmall
    readonly property int spacingMedium: ThemeViewModel.tokens.spacingMedium
    readonly property int spacingLarge: ThemeViewModel.tokens.spacingLarge

    // 通用图标尺寸（逻辑像素）
    readonly property int iconSizeSmall: ThemeViewModel.tokens.iconSizeSmall
    readonly property int iconSizeDefault: ThemeViewModel.tokens.iconSizeDefault
    readonly property int iconSizeCompact: ThemeViewModel.tokens.iconSizeCompact

    // 圆角与边框（逻辑像素）
    readonly property int radiusSmall: ThemeViewModel.tokens.radiusSmall
    readonly property int borderWidth: ThemeViewModel.tokens.borderWidth
    readonly property int focusBorderWidth: ThemeViewModel.tokens.focusBorderWidth

    // 通用控件尺寸（逻辑像素）
    readonly property int comboBoxHeight: ThemeViewModel.tokens.comboBoxHeight
    readonly property int controlHeight: ThemeViewModel.tokens.controlHeight
    readonly property int selectionIndicatorSize: ThemeViewModel.tokens.selectionIndicatorSize
    readonly property int toolbarButtonHeight: ThemeViewModel.tokens.toolbarButtonHeight
    readonly property int paneCloseSize: ThemeViewModel.tokens.paneCloseSize

    // Shell 条带与标题布局（逻辑像素）
    readonly property int titlebarHeight: ThemeViewModel.tokens.titlebarHeight
    readonly property int menubarHeight: ThemeViewModel.tokens.menubarHeight
    readonly property int statusbarHeight: ThemeViewModel.tokens.statusbarHeight

    // 弹窗与表单控件（逻辑像素）
    readonly property int dialogActionWidth: ThemeViewModel.tokens.dialogActionWidth
    readonly property int dialogActionSpacing: ThemeViewModel.tokens.dialogActionSpacing
    readonly property int settingsDialogWidth: ThemeViewModel.tokens.settingsDialogWidth
    readonly property int settingsDialogHeight: ThemeViewModel.tokens.settingsDialogHeight
    readonly property int settingsDialogSidebarWidth: ThemeViewModel.tokens.settingsDialogSidebarWidth
    readonly property int formControlHeight: ThemeViewModel.tokens.formControlHeight

    // 悬停提示延迟（毫秒）
    readonly property int toolTipDelay: ThemeViewModel.tokens.toolTipDelay

    // 禁用状态
    readonly property real disabledOpacity: ThemeViewModel.tokens.disabledOpacity

    // 新建文档窗口尺寸与最小约束（逻辑像素）
    readonly property int newProjectDialogWidth: ThemeViewModel.tokens.newProjectDialogWidth
    readonly property int newProjectDialogHeight: ThemeViewModel.tokens.newProjectDialogHeight
    readonly property int newProjectDialogMinimumWidth: ThemeViewModel.tokens.newProjectDialogMinimumWidth
    readonly property int newProjectDialogMinimumHeight: ThemeViewModel.tokens.newProjectDialogMinimumHeight
}
