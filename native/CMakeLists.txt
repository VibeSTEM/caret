# native 入口。来源 panta ee84d95 的构建策略，裁掉几何、网格和工艺模块。
# 3.22 是本树用到的最低版本，不随在线文档的最新版浮动。
cmake_minimum_required(VERSION 3.22)
project(
  caret_native
  VERSION 0.1.0
  LANGUAGES C CXX)

option(CARET_ENABLE_APP "Link caret-ffi and build the blank desktop window" OFF)
option(CARET_ENABLE_COVERAGE "Instrument native C++ targets for LLVM coverage" OFF)
option(CARET_USE_SYSTEM_TOOLS "Use explicitly selected host tools instead of managed tools" OFF)
set(CARET_LLVM_VERSION
    ""
    CACHE STRING "Expected managed LLVM version")
set(CARET_SANITIZER
    ""
    CACHE STRING "Comma-separated sanitizers: address, undefined, thread")
set(CARET_I18N_TS_DIR
    ""
    CACHE PATH "Directory of TS files generated from resources/i18n")
set(CARET_FFI_INCLUDE_DIR
    ""
    CACHE PATH "Directory containing caret_ffi.h")
set(CARET_FFI_STATIC_LIB
    ""
    CACHE FILEPATH "caret-ffi static library produced by Cargo")

include(cmake/build-policy.cmake)
include(cmake/layering.cmake)
cmake_language(DEFER CALL caret_verify_layering)
include(CTest)
include(cmake/sdk-provision.cmake)
if(BUILD_TESTING)
  add_test(NAME Build.SdkProvision
           COMMAND "${CMAKE_COMMAND}" "-DCARET_SDK_FIXTURE_ROOT=${CMAKE_BINARY_DIR}/sdk-fixtures"
                   -P "${CMAKE_CURRENT_SOURCE_DIR}/cmake/tests/check-sdk-provision.cmake")
  set_tests_properties(Build.SdkProvision PROPERTIES LABELS "build" TIMEOUT 120)
endif()

include(cmake/qt-provision.cmake)
set(QT_HOST_PATH "${QT_STAGING}")
set(_caret_qt_components
    Core
    Gui
    Qml
    Quick
    QuickControls2
    QuickDialogs2
    Svg)
if(BUILD_TESTING)
  list(APPEND _caret_qt_components Test)
endif()
find_package(Qt6 6.11 REQUIRED COMPONENTS ${_caret_qt_components} PATHS "${QT_STAGING}"
                                          NO_DEFAULT_PATH)
if(CARET_ENABLE_APP AND UNIX AND NOT APPLE)
  find_package(PkgConfig REQUIRED)
  pkg_check_modules(Fontconfig REQUIRED IMPORTED_TARGET fontconfig)
endif()
qt_standard_project_setup()
qt_policy(SET QTP0001 NEW)
qt_policy(SET QTP0004 NEW)
qt_policy(SET QTP0005 NEW)

if(CARET_I18N_TS_DIR)
  add_subdirectory(i18n)
endif()

add_subdirectory(canvas)
add_subdirectory(viewmodels)
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../qml" qml)

if(CARET_ENABLE_APP)
  if(NOT EXISTS "${CARET_FFI_INCLUDE_DIR}/caret_ffi.h"
     OR NOT EXISTS "${CARET_FFI_STATIC_LIB}"
     OR IS_DIRECTORY "${CARET_FFI_STATIC_LIB}")
    message(
      FATAL_ERROR
        "CARET_ENABLE_APP requires CARET_FFI_INCLUDE_DIR/caret_ffi.h and CARET_FFI_STATIC_LIB; build via Cargo"
    )
  endif()
  target_compile_definitions(caret_viewmodels PRIVATE CARET_ENABLE_APP=1)
  target_include_directories(caret_viewmodels PRIVATE "${CARET_FFI_INCLUDE_DIR}")
  target_link_libraries(caret_viewmodels PRIVATE "${CARET_FFI_STATIC_LIB}")
  add_subdirectory(app)
endif()

if(MSVC)
  cmake_language(DEFER CALL caret_verify_msvc_abi "${CMAKE_SOURCE_DIR}")
endif()
