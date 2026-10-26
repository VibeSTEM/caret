# 桌面入口。链接已经生成的 caret-ffi staticlib，不回调 Cargo。
qt_add_executable(caret_native_app MANUAL_FINALIZATION main.cpp theme_catalog.cpp)
set_target_properties(caret_native_app PROPERTIES OUTPUT_NAME caret EXPORT_NAME native-app)
if(WIN32)
  set_target_properties(
    caret_native_app PROPERTIES QT_WINDOWS_APP_PROJECT_IDENTIFIER "com.vibestem.caret"
                                QT_WINDOWS_APP_PROJECT_EXECUTION_LEVEL "asInvoker")
endif()
if(APPLE)
  set_target_properties(
    caret_native_app
    PROPERTIES MACOSX_BUNDLE TRUE
               MACOSX_BUNDLE_GUI_IDENTIFIER "com.vibestem.caret"
               MACOSX_BUNDLE_BUNDLE_NAME "Caret")
endif()
caret_native_defaults(caret_native_app)
target_link_libraries(
  caret_native_app
  PRIVATE caret::shell
          caret_shellplugin
          caret::canvas
          caret_canvasplugin
          caret::viewmodels
          caret_viewmodelsplugin
          "${CARET_FFI_STATIC_LIB}"
          Qt6::Gui
          Qt6::Qml
          Qt6::Quick
          Qt6::QuickControls2)
target_include_directories(caret_native_app PRIVATE "${CARET_FFI_INCLUDE_DIR}")
if(TARGET caret_i18n)
  target_link_libraries(caret_native_app PRIVATE caret_i18n)
endif()
if(UNIX)
  find_package(Threads REQUIRED)
  target_link_libraries(caret_native_app PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
elseif(WIN32)
  target_link_libraries(caret_native_app PRIVATE ws2_32 userenv ntdll)
endif()
# QML 实际位于静态 shell 模块，而不是本目录；显式提供扫描根和文件依赖。
# 否则安装脚本得到空 imports 清单，开发机可运行而可搬迁包缺少 QtQuick 模块。
get_target_property(_caret_shell_qml_files caret_shell QT_QML_MODULE_QML_FILES)
set_target_properties(
  caret_native_app
  PROPERTIES QT_QML_IMPORT_SCANNER_EXTRA_ROOT_PATHS "${CMAKE_CURRENT_SOURCE_DIR}/../../qml"
             QT_QML_MODULE_QML_FILES "${_caret_shell_qml_files}")
qt_finalize_executable(caret_native_app)

if(WIN32)
  get_filename_component(_caret_sdk_bin "${CMAKE_RC_COMPILER}" DIRECTORY)
  find_program(
    CARET_WINDOWS_MANIFEST_TOOL
    NAMES mt.exe
    PATHS "${_caret_sdk_bin}"
    NO_DEFAULT_PATH REQUIRED)
  function(_caret_validate_windows_manifest)
    get_target_property(_caret_finalized caret_native_app _qt_is_finalized)
    if(NOT _caret_finalized)
      # Qt 可能将 target finalization 延后到 QML 文件生成之后；验证必须排在它之后。
      get_property(
        _caret_retry_count
        DIRECTORY
        PROPERTY _caret_manifest_validation_retries)
      if(NOT _caret_retry_count)
        set(_caret_retry_count 0)
      endif()
      if(_caret_retry_count GREATER_EQUAL 8)
        message(FATAL_ERROR "Qt 未能完成 caret_native_app finalization，无法验证 Windows manifest")
      endif()
      math(EXPR _caret_retry_count "${_caret_retry_count} + 1")
      set_property(DIRECTORY PROPERTY _caret_manifest_validation_retries "${_caret_retry_count}")
      cmake_language(DEFER CALL _caret_validate_windows_manifest)
      return()
    endif()

    set(_caret_windows_manifest "${CMAKE_CURRENT_BINARY_DIR}/caret.exe.manifest")
    if(NOT EXISTS "${_caret_windows_manifest}")
      message(FATAL_ERROR "Qt 未生成预期的 Windows 应用清单：${_caret_windows_manifest}")
    endif()
    get_target_property(_caret_app_sources caret_native_app SOURCES)
    set(_caret_manifest_sources "${_caret_app_sources}")
    list(FILTER _caret_manifest_sources INCLUDE REGEX "\\.manifest$")
    list(LENGTH _caret_manifest_sources _caret_manifest_source_count)
    if(NOT _caret_windows_manifest IN_LIST _caret_manifest_sources
       OR NOT _caret_manifest_source_count EQUAL 1)
      message(
        FATAL_ERROR
          "Windows 应用目标必须且只能引用二进制目录中的清单：${_caret_windows_manifest}；实际引用：${_caret_manifest_sources}")
    endif()
    # CMake 的 VS 链接包装会把用户清单作为 /MANIFESTINPUT 交给 LLD，
    # 其内部合并会改写 UAC 属性命名空间。链接阶段排除清单，再由 SDK mt.exe 嵌入。
    list(REMOVE_ITEM _caret_app_sources "${_caret_windows_manifest}")
    set_property(TARGET caret_native_app PROPERTY SOURCES "${_caret_app_sources}")
    # 清单是链接后的资源输入；修改应用标识或 UAC 设置时必须重链并重新嵌入。
    set_property(
      TARGET caret_native_app
      APPEND
      PROPERTY LINK_DEPENDS "${_caret_windows_manifest}")
    add_custom_command(
      TARGET caret_native_app
      POST_BUILD
      COMMAND "${CARET_WINDOWS_MANIFEST_TOOL}" -nologo -manifest "${_caret_windows_manifest}"
              "-outputresource:$<TARGET_FILE:caret_native_app>;#1"
      COMMENT "嵌入 Windows 应用清单"
      VERBATIM)
  endfunction()
  cmake_language(DEFER CALL _caret_validate_windows_manifest)
endif()

include(../cmake/desktop-deployment.cmake)
