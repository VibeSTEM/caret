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
qt_finalize_executable(caret_native_app)
