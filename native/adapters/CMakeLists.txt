# Caret.Adapters。仅连接 Qt 和平台 API，不依赖 Rust、画布或 ViewModel。
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")
if(UNIX AND NOT APPLE)
  set(CARET_QML_MODULE_OPTIONS __QT_INTERNAL_NO_GENERATE_QMLLS_BUILD_INI)
endif()

qt_add_qml_module(
  caret_adapters
  STATIC
  URI
  Caret.Adapters
  VERSION
  1.0
  ${CARET_QML_MODULE_OPTIONS}
  RESOURCE_PREFIX
  /qt/qml
  SOURCES
  platform_fonts.hpp
  platform_fonts.cpp
  document_path_adapter.hpp
  document_path_adapter.cpp
  window_chrome_metrics.hpp
  window_chrome_metrics.cpp
  icon_provider.hpp
  icon_provider.cpp)
if(APPLE)
  target_sources(caret_adapters PRIVATE window_chrome_metrics_mac.mm)
endif()
set_target_properties(caret_adapters PROPERTIES EXPORT_NAME adapters)
caret_native_defaults(caret_adapters)
add_library(caret::adapters ALIAS caret_adapters)
target_include_directories(caret_adapters PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(caret_adapters PRIVATE Qt6::Svg Qt6::Quick)
