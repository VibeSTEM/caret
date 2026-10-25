# GoogleTest SDK 生产入口；来源 panta ee84d95 的 sdk-googletest 工作流。
# 只供手动 SDK 生产使用，普通 Cargo 构建不会进入本目录。
cmake_minimum_required(VERSION 3.22)
project(caret_googletest_sdk LANGUAGES CXX)
if(NOT EXISTS "${GOOGLETEST_SOURCE}/CMakeLists.txt" OR NOT SDK_TRIPLE)
  message(FATAL_ERROR "需要 GOOGLETEST_SOURCE 和 SDK_TRIPLE")
endif()
file(TO_CMAKE_PATH "${GOOGLETEST_SOURCE}" GOOGLETEST_SOURCE)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")
if(WIN32)
  add_compile_definitions(_ITERATOR_DEBUG_LEVEL=0)
endif()
set(BUILD_SHARED_LIBS
    OFF
    CACHE BOOL "Static SDK" FORCE)
set(BUILD_GMOCK
    OFF
    CACHE BOOL "GoogleTest only" FORCE)
set(INSTALL_GTEST
    ON
    CACHE BOOL "Install CONFIG package" FORCE)
set(gtest_force_shared_crt
    ON
    CACHE BOOL "Use release dynamic CRT" FORCE)
add_subdirectory("${GOOGLETEST_SOURCE}" googletest)
install(FILES "${GOOGLETEST_SOURCE}/LICENSE" DESTINATION share/licenses/GoogleTest)
file(
  WRITE "${CMAKE_CURRENT_BINARY_DIR}/caret-sdk.json"
  "{\n  \"name\": \"googletest\",\n  \"version\": \"1.18.0\",\n  \"triple\": \"${SDK_TRIPLE}\",\n  \"source_commit\": \"063de7e9578f82b369302001269680b4b1553359\",\n  \"llvm\": \"22.1.7\",\n  \"cxx_standard\": 20,\n  \"shared\": false,\n  \"windows_shared_crt\": true\n}\n"
)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/caret-sdk.json" DESTINATION .)
