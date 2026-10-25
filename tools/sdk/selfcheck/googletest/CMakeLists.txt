# GoogleTest SDK 自检：通过归档内 CONFIG package 编译、静态链接并运行一个测试。

cmake_minimum_required(VERSION 3.22)
project(caret_googletest_sdk_selfcheck LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")
if(WIN32)
  add_compile_definitions(_ITERATOR_DEBUG_LEVEL=0)
endif()
include(CTest)

if(NOT DEFINED PSDK_ROOT)
  message(FATAL_ERROR "selfcheck 需要 -DPSDK_ROOT 指向 SDK 安装树")
endif()

file(TO_CMAKE_PATH "${PSDK_ROOT}" PSDK_ROOT)

foreach(_required IN ITEMS caret-sdk.json share/licenses/GoogleTest/LICENSE)
  if(NOT EXISTS "${PSDK_ROOT}/${_required}")
    message(FATAL_ERROR "GoogleTest SDK 缺少 ${_required}：${PSDK_ROOT}/${_required}")
  endif()
endforeach()

find_package(
  GTest
  1.18.0
  EXACT
  CONFIG
  REQUIRED
  PATHS
  "${PSDK_ROOT}"
  NO_DEFAULT_PATH)
foreach(_target IN ITEMS GTest::gtest GTest::gtest_main)
  if(NOT TARGET "${_target}")
    message(FATAL_ERROR "GoogleTest SDK 缺少 imported target ${_target}")
  endif()
endforeach()

add_executable(googletest_sdk_smoke main.cpp)
target_link_libraries(googletest_sdk_smoke PRIVATE GTest::gtest_main)
add_test(NAME GoogleTest.SdkSmoke COMMAND googletest_sdk_smoke)
