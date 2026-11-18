# Caret.ViewModels。按设置、主题和文档分类发布 Rust 的只读 Qt 投影。
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")
set(CARET_COMMAND_LINE_PROTOCOL_JSON
    "${CMAKE_CURRENT_SOURCE_DIR}/../../shared/command_line_protocol.json")
file(READ "${CARET_COMMAND_LINE_PROTOCOL_JSON}" CARET_COMMAND_LINE_PROTOCOL_SOURCE)
file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/generated")
file(
  WRITE "${CMAKE_CURRENT_BINARY_DIR}/generated/command_line_protocol_json.hpp"
  "#pragma once\nnamespace caret::layout_snapshot {\ninline constexpr char kCommandLineProtocolJson[] = R\"caretprotocol(${CARET_COMMAND_LINE_PROTOCOL_SOURCE})caretprotocol\";\n}\n"
)
set_property(
  DIRECTORY
  APPEND
  PROPERTY CMAKE_CONFIGURE_DEPENDS "${CARET_COMMAND_LINE_PROTOCOL_JSON}")
if(UNIX AND NOT APPLE)
  set(CARET_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

qt_add_qml_module(
  caret_viewmodels
  STATIC
  URI
  Caret.ViewModels
  VERSION
  1.0
  ${CARET_QML_MODULE_OPTIONS}
  RESOURCE_PREFIX
  /qt/qml
  SOURCES
  document/layout_view_model.hpp
  document/layout_view_model.cpp
  document/layout_snapshot_codec.hpp
  document/layout_snapshot_codec.cpp
  document/layout_view_model_clipboard_commands.cpp
  document/layout_view_model_file_commands.cpp
  document/layout_view_model_recovery_commands.cpp
  document/layout_view_model_autosave_commands.cpp
  document/layout_view_model_edit_commands.cpp
  document/layout_view_model_command_line.cpp
  document/layout_view_model_ime_commands.cpp
  document/layout_view_model_navigation_commands.cpp
  document/layout_view_model_response_commands.cpp
  document/layout_view_model_selection_commands.cpp
  document/glyph_frame_adapter.cpp
  document/glyph_frame_adapter.hpp
  document/document_runtime_bridge.hpp
  document/document_runtime_bridge.cpp
  settings/preferences_view_model.hpp
  settings/preferences_view_model.cpp
  settings/application_translator.hpp
  settings/application_translator.cpp
  settings/settings_view_model.hpp
  settings/settings_view_model.cpp
  settings/settings_runtime_bridge.hpp
  settings/settings_runtime_bridge.cpp
  settings/settings_dialog_controller.hpp
  settings/settings_dialog_controller.cpp
  theme/theme_view_model.hpp
  theme/theme_view_model.cpp
  theme/theme_catalog.hpp
  theme/theme_catalog.cpp)
set_target_properties(caret_viewmodels PROPERTIES EXPORT_NAME viewmodels)
caret_native_defaults(caret_viewmodels)
add_library(caret::viewmodels ALIAS caret_viewmodels)
target_include_directories(
  caret_viewmodels
  PUBLIC document settings theme "${CMAKE_CURRENT_SOURCE_DIR}/.."
  PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/generated")
target_link_libraries(caret_viewmodels PUBLIC Qt6::Concurrent Qt6::Core Qt6::Qml Qt6::StateMachine)
