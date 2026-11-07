# Caret.Shell。qt_add_qml_module 与 qml/ 目录放在一起。模块不链接 Rust。

set_source_files_properties(Themes/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")

set(CARET_SHELL_QML_FILES
    App.qml
    WindowTitleBar.qml
    WindowCommandBar.qml
    PageSurface.qml
    WindowStatusBar.qml
    Themes/Theme.qml
    Components/Atoms/CommandBarTabButton.qml
    Components/Atoms/ThemedNumericField.qml
    Components/Atoms/ThemedButton.qml
    Components/Atoms/ThemedCheckBox.qml
    Components/Atoms/ThemedSwitch.qml
    Components/Atoms/TitleBarPillButton.qml
    Components/Atoms/CommandCandidateItem.qml
    Components/Atoms/AutoScrollText.qml
    Components/Atoms/HighlightBackground.qml
    Components/Atoms/ScrollableListView.qml
    Components/Atoms/ThemedRadioButton.qml
    Components/Atoms/ThemedComboBox.qml
    Components/Atoms/ThemedLabel.qml
    Components/Atoms/ThemedToolButton.qml
    Components/Atoms/ThemedTextField.qml
    Components/Atoms/ThemedIcon.qml
    Canvas/DocumentCanvasPointerHandler.qml
    Canvas/CommandCompletion.qml
    Canvas/CommandPopupFrame.qml
    Canvas/CommandSuggestionPopup.qml
    Canvas/FontPickerPopup.qml
    Canvas/CommandInputHandler.qml
    Components/Composites/DocumentModeSelector.qml
    Components/Composites/ShareButton.qml
    Components/Composites/DialogTitleBar.qml
    Components/Composites/DialogWindow.qml
    Components/Composites/DialogFrame.qml
    Components/Composites/DialogButtonRow.qml
    Dialogs/NewProjectDialog.qml
    Dialogs/RecoveryDialog.qml
    Dialogs/SaveConflictDialog.qml
    Dialogs/UnsavedChangesDialog.qml
    Dialogs/SettingsDialog.qml
    Dialogs/LanguageSettingsPage.qml
    Dialogs/ThemeSettingsPage.qml
    Dialogs/PreferencesSettingsPage.qml)

if(UNIX AND NOT APPLE)
  # Linux 上 qtpaths 与 ICU 73 的 qmlls 辅助文件可以跳过，不影响资源和 typeinfo。
  set(CARET_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

qt_add_qml_module(
  caret_shell
  STATIC
  URI
  Caret.Shell
  VERSION
  1.0
  DEPENDENCIES
  Caret.Adapters
  Caret.Canvas
  Caret.ViewModels
  RESOURCE_PREFIX
  /qt/qml
  ${CARET_QML_MODULE_OPTIONS}
  QML_FILES
  ${CARET_SHELL_QML_FILES})
set_target_properties(caret_shell PROPERTIES EXPORT_NAME shell)
caret_native_defaults(caret_shell)
add_library(caret::shell ALIAS caret_shell)
target_link_libraries(caret_shell PRIVATE caret::adapters Qt6::QuickDialogs2)

qt_add_resources(
  caret_shell
  caret_shell_themes
  PREFIX
  "/qt/qml/Caret/Shell"
  FILES
  Themes/gui-default.pa
  Themes/light.pa
  Themes/dark.pa)

set(CARET_SHELL_ICON_FILES
    icons/caret.svg
    icons/check.svg
    icons/close.svg
    icons/comments.svg
    icons/mode-edit.svg
    icons/mode-review.svg
    icons/mode-view.svg
    icons/open.svg
    icons/redo.svg
    icons/right.svg
    icons/save.svg
    icons/search.svg
    icons/share.svg
    icons/settings.svg
    icons/undo.svg
    icons/zoom-in.svg
    icons/zoom-out.svg)
qt_add_resources(caret_shell "caret_shell_icons" PREFIX "/qt/qml/Caret/Shell" FILES
                 ${CARET_SHELL_ICON_FILES})

if(BUILD_TESTING)
  add_executable(
    caret_code_font_failure_test
    ../native/adapters/fonts/code.cpp ../native/adapters/fonts/code.hpp
    ../tests/cpp/shell/code_font_failure_test.cpp)
  qt_add_resources(
    caret_code_font_failure_test
    caret_code_fonts
    PREFIX
    "/caret/fonts"
    BASE
    "${CMAKE_CURRENT_SOURCE_DIR}/../resources/fonts/code/jetbrains-mono"
    FILES
    "${CMAKE_CURRENT_SOURCE_DIR}/../resources/fonts/code/jetbrains-mono/JetBrainsMono-Regular.ttf")
  caret_native_defaults(caret_code_font_failure_test)
  target_include_directories(caret_code_font_failure_test
                             PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../native/adapters/fonts")
  target_link_libraries(caret_code_font_failure_test PRIVATE Qt6::Gui Qt6::Test)
  add_test(NAME caret_code_font_failure_test COMMAND caret_code_font_failure_test)
  set(_caret_code_font_failure_test_environment
      "QT_QPA_PLATFORM=offscreen;QT_PLUGIN_PATH=${QT_STAGING}/plugins;QT_FORCE_STDERR_LOGGING=1")
  if(UNIX
     AND NOT APPLE
     AND "address" IN_LIST _caret_sanitizers)
    # 只抑制这个字体注册测试中 Qt/Fontconfig 初始化栈的系统分配报告。
    list(
      APPEND
      _caret_code_font_failure_test_environment
      "LSAN_OPTIONS=suppressions=${CMAKE_CURRENT_SOURCE_DIR}/../tests/fixtures/sanitizers/linux-fontconfig-lsan.supp"
    )
  endif()
  set_tests_properties(caret_code_font_failure_test
                       PROPERTIES ENVIRONMENT "${_caret_code_font_failure_test_environment}")

  add_executable(
    caret_shell_load_test
    ../tests/cpp/shell/shell_load_test.cpp
    ../tests/cpp/shell/shell_load_test.hpp
    ../tests/cpp/shell/shell_document_path_test.cpp
    ../tests/cpp/shell/shell_window_chrome_test.cpp
    ../tests/cpp/shell/shell_document_canvas_test.cpp
    ../tests/cpp/shell/shell_document_interaction_test.cpp
    ../tests/cpp/shell/shell_document_file_test.cpp
    ../tests/cpp/shell/shell_document_navigation_test.cpp
    ../tests/cpp/shell/shell_layout_snapshot_validation_test.cpp
    ../tests/cpp/shell/shell_document_autosave_test.cpp
    ../tests/cpp/shell/shell_document_recovery_test.cpp
    ../tests/cpp/shell/shell_theme_catalog_test.cpp)
  caret_native_defaults(caret_shell_load_test)
  target_compile_definitions(
    caret_shell_load_test PRIVATE "CARET_SOURCE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/..\""
                                  "CARET_QML_IMPORT_PATH=\"${QT_QML_OUTPUT_DIRECTORY}\"")
  target_link_libraries(
    caret_shell_load_test
    PRIVATE caret_adapters
            caret_adaptersplugin
            caret_shell
            caret_shellplugin
            caret_canvas
            caret_canvasplugin
            caret_viewmodels
            caret_viewmodelsplugin
            Qt6::Gui
            Qt6::Qml
            Qt6::Quick
            Qt6::QuickControls2
            Qt6::Test)
  if(CARET_ENABLE_APP)
    target_compile_definitions(caret_shell_load_test PRIVATE CARET_ENABLE_APP=1)
    target_include_directories(caret_shell_load_test PRIVATE "${CARET_FFI_INCLUDE_DIR}")
    target_link_libraries(caret_shell_load_test PRIVATE "${CARET_FFI_STATIC_LIB}")
    if(TARGET caret_i18n)
      target_link_libraries(caret_shell_load_test PRIVATE caret_i18n)
    endif()
    if(UNIX)
      find_package(Threads REQUIRED)
      target_link_libraries(caret_shell_load_test PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
      if(TARGET PkgConfig::Fontconfig)
        target_link_libraries(caret_shell_load_test PRIVATE PkgConfig::Fontconfig)
      endif()
    elseif(WIN32)
      target_link_libraries(caret_shell_load_test PRIVATE ws2_32 userenv ntdll)
    endif()
  endif()
  caret_add_qml_test(Qml.ShellLoad caret_shell_load_test)
  # 该聚合测试在 Ubuntu CI 超过通用 20 秒阈值；只为 ShellLoad 留出运行余量。
  if(NOT _caret_sanitizers)
    set_tests_properties(Qml.ShellLoad PROPERTIES TIMEOUT 45)
  endif()
  if(CARET_ENABLE_APP)
    caret_add_qml_test(Qml.PreferencesWithoutRuntime caret_shell_load_test qml
                       preferencesWithoutRuntimeRejectChangesAndKeepDefaults)
  endif()

  find_program(
    CARET_QMLFORMAT_EXECUTABLE
    NAMES qmlformat
    PATHS "${QT_STAGING}/bin"
    NO_DEFAULT_PATH REQUIRED)
  add_test(
    NAME Qml.FormatCheck
    COMMAND
      "${CMAKE_COMMAND}" "-DQMLFMT=${CARET_QMLFORMAT_EXECUTABLE}"
      "-DQML_DIR=${CMAKE_CURRENT_SOURCE_DIR}" -P
      "${CMAKE_CURRENT_SOURCE_DIR}/../native/cmake/tests/check-qml-format.cmake")
  set_tests_properties(Qml.FormatCheck PROPERTIES LABELS "qml")
endif()
