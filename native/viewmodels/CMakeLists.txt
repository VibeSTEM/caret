# Caret.ViewModels。Rust 快照到 Qt/QML 属性的 N2 适配模块。
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/..")
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
  ../app/layout_view_model.hpp
  ../app/layout_view_model.cpp
  ../app/preferences_view_model.hpp
  ../app/preferences_view_model.cpp)
set_target_properties(caret_viewmodels PROPERTIES EXPORT_NAME viewmodels)
caret_native_defaults(caret_viewmodels)
add_library(caret::viewmodels ALIAS caret_viewmodels)
target_include_directories(caret_viewmodels PUBLIC ../app)
target_link_libraries(caret_viewmodels PRIVATE Qt6::Core Qt6::Qml)
