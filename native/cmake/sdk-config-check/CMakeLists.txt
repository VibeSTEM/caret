# SDK 发布前的隔离 CONFIG 探针。不能在主工程加载 staging 下的 imported targets，
# 否则目录替换后主工程仍会引用已经不存在的 staging 路径。
cmake_minimum_required(VERSION 3.22)
project(caret_gtest_config_check LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
if(WIN32)
  add_compile_definitions(_ITERATOR_DEBUG_LEVEL=0)
endif()
if(NOT CARET_GTEST_CHECK_ROOT)
  message(FATAL_ERROR "需要 CARET_GTEST_CHECK_ROOT")
endif()
if(EXISTS "${CARET_GTEST_CHECK_ROOT}/lib/cmake/GTest/GTestConfig.cmake")
  set(GTest_DIR "${CARET_GTEST_CHECK_ROOT}/lib/cmake/GTest")
else()
  set(GTest_DIR "${CARET_GTEST_CHECK_ROOT}/lib64/cmake/GTest")
endif()
find_package(
  GTest
  1.18.0
  EXACT
  CONFIG
  REQUIRED
  PATHS
  "${GTest_DIR}"
  NO_DEFAULT_PATH)
if(NOT TARGET GTest::gtest OR NOT TARGET GTest::gtest_main)
  message(FATAL_ERROR "GoogleTest SDK CONFIG 未提供 GTest::gtest 和 GTest::gtest_main")
endif()
