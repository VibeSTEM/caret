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
  canvas_policy.hpp
  document_units.hpp
  document_units.cpp
  document_canvas.hpp
  document_canvas.cpp
  document_canvas/caret.cpp
  document_canvas/geometry.cpp
  document_canvas/glyph_frames.cpp
  document_canvas/input.cpp
  document_canvas/keyboard.cpp
  document_canvas/selection.cpp
  document_canvas/structure_focus.cpp
  document_canvas/scene_graph.cpp
  document_canvas/value_validation.hpp)
set_target_properties(caret_canvas PROPERTIES EXPORT_NAME canvas)
caret_native_defaults(caret_canvas)
add_library(caret::canvas ALIAS caret_canvas)
target_include_directories(caret_canvas PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(caret_canvas PRIVATE Qt6::Gui Qt6::Qml Qt6::Quick)

if(BUILD_TESTING AND CARET_ENABLE_PERFORMANCE)
  add_executable(caret_qt_cpu_benchmark EXCLUDE_FROM_ALL
                 ../../tests/performance/qt/cpu/software_canvas_benchmark.cpp)
  target_include_directories(caret_qt_cpu_benchmark
                             PRIVATE "${CMAKE_SOURCE_DIR}/../tests/performance/qt")
  target_link_libraries(caret_qt_cpu_benchmark PRIVATE caret_canvas Qt6::Gui Qt6::Quick)
  caret_native_defaults(caret_qt_cpu_benchmark)

  add_executable(caret_qt_gpu_benchmark EXCLUDE_FROM_ALL
                 ../../tests/performance/qt/gpu/headless_canvas_benchmark.cpp)
  target_include_directories(caret_qt_gpu_benchmark
                             PRIVATE "${CMAKE_SOURCE_DIR}/../tests/performance/qt")
  target_link_libraries(caret_qt_gpu_benchmark PRIVATE caret_canvas Qt6::Gui Qt6::Quick
                                                       Qt6::GuiPrivate)
  caret_native_defaults(caret_qt_gpu_benchmark)
endif()
