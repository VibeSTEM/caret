# Caret.Canvas。多页绘制与页面坐标映射作为独立 N2 QML 模块提供。
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")
if(UNIX AND NOT APPLE)
  set(CARET_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

qt_add_qml_module(
  caret_canvas
  STATIC
  URI
  Caret.Canvas
  VERSION
  1.0
  ${CARET_QML_MODULE_OPTIONS}
  RESOURCE_PREFIX
  /qt/qml
  SOURCES
  document_canvas.hpp
  document_canvas.cpp)
set_target_properties(caret_canvas PROPERTIES EXPORT_NAME canvas)
caret_native_defaults(caret_canvas)
add_library(caret::canvas ALIAS caret_canvas)
target_include_directories(caret_canvas PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(caret_canvas PRIVATE Qt6::Gui Qt6::Qml Qt6::Quick)
