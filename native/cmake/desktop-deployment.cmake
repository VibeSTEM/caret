# Runtime 安装和 Qt 部署来自 panta ee84d95，裁掉领域 SDK 和开发期导出。
# 资源在 Qt 部署前写入 bundle，避免使 macOS 部署后的签名失效。
install(
  TARGETS caret_native_app
  BUNDLE DESTINATION . COMPONENT Runtime
  RUNTIME DESTINATION bin COMPONENT Runtime)
if(APPLE)
  set(_caret_resources "caret.app/Contents/Resources")
else()
  set(_caret_resources "share/caret")
  if(UNIX)
    set_property(TARGET caret_native_app PROPERTY INSTALL_RPATH "$ORIGIN/../lib")
  endif()
endif()
string(REPLACE "\\" "/" _caret_qt_staging "${QT_STAGING}")
install(
  DIRECTORY "${_caret_qt_staging}/sbom/"
  DESTINATION "${_caret_resources}/sbom"
  COMPONENT Runtime
  FILES_MATCHING
  PATTERN "*.spdx.json"
  PATTERN "*.cdx.json")
# package runner 先聚合本项目和 Cargo 依赖的许可文件，缺失时安装失败。
install(
  DIRECTORY "${CMAKE_BINARY_DIR}/package-licenses/"
  DESTINATION "${_caret_resources}/licenses"
  COMPONENT Runtime)
qt_generate_deploy_qml_app_script(TARGET caret_native_app OUTPUT_SCRIPT _caret_deploy_script
                                  NO_TRANSLATIONS)
install(SCRIPT "${_caret_deploy_script}" COMPONENT Runtime)
