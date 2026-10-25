if(NOT CARET_SDK_FIXTURE_ROOT)
  message(FATAL_ERROR "需要 CARET_SDK_FIXTURE_ROOT")
endif()
set(_fixture "${CMAKE_CURRENT_LIST_DIR}/sdk/consumer")
file(REMOVE_RECURSE "${CARET_SDK_FIXTURE_ROOT}")
file(MAKE_DIRECTORY "${CARET_SDK_FIXTURE_ROOT}")
set(_root "${CARET_SDK_FIXTURE_ROOT}")
set(_triple "macos-arm64")

# CONFIG 查找夹具只验证供给边界；真实静态链接另由 SDK 生产自检覆盖。
set(_sdk "${_root}/local-sdk")
file(MAKE_DIRECTORY "${_sdk}/lib/cmake/GTest")
file(
  WRITE "${_sdk}/caret-sdk.json"
  "{\"name\":\"googletest\",\"version\":\"1.18.0\",\"triple\":\"${_triple}\",\"llvm\":\"22.1.7\",\"cxx_standard\":20,\"shared\":false,\"windows_shared_crt\":true}"
)
file(
  WRITE "${_sdk}/lib/cmake/GTest/GTestConfig.cmake"
  "add_library(GTest::gtest INTERFACE IMPORTED)\nadd_library(GTest::gtest_main INTERFACE IMPORTED)\n"
)
file(
  WRITE "${_sdk}/lib/cmake/GTest/GTestConfigVersion.cmake"
  "set(PACKAGE_VERSION 1.18.0)\nif(PACKAGE_FIND_VERSION STREQUAL PACKAGE_VERSION)\n  set(PACKAGE_VERSION_EXACT TRUE)\n  set(PACKAGE_VERSION_COMPATIBLE TRUE)\nendif()\n"
)

function(check_case name expect_success expected_diagnostic)
  execute_process(
    COMMAND
      "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_root}/${name}" -DBUILD_TESTING=ON
      -DCARET_FIXTURE_REQUEST_GTEST=ON -DCARET_TARGET_TRIPLE=aarch64-apple-darwin
      "-DCARET_TOOLCHAIN_ROOT=${_root}/${name}-toolchain" ${ARGN}
    RESULT_VARIABLE _result
    OUTPUT_VARIABLE _stdout
    ERROR_VARIABLE _stderr)
  set(_log "${_stdout}\n${_stderr}")
  string(REGEX REPLACE "[ \r\n\t]+" " " _log "${_log}")
  if(expect_success AND NOT _result EQUAL 0)
    message(FATAL_ERROR "${name} 应成功却失败：${_log}")
  elseif(NOT expect_success AND _result EQUAL 0)
    message(FATAL_ERROR "${name} 应拒绝却成功")
  elseif(NOT expect_success AND NOT _log MATCHES "${expected_diagnostic}")
    message(FATAL_ERROR "${name} 未给出预期诊断 '${expected_diagnostic}'：${_log}")
  endif()
endfunction()

check_case(disabled TRUE "" -DBUILD_TESTING=OFF
           -DCARET_GTEST_URL=https://invalid.invalid/no-download)
check_case(unrequested TRUE "" -DCARET_FIXTURE_REQUEST_GTEST=OFF)
foreach(_case disabled unrequested)
  if(EXISTS "${_root}/${_case}-toolchain")
    message(FATAL_ERROR "${_case} 意外触发 SDK 目录创建或下载")
  endif()
endforeach()
check_case(unpinned FALSE "尚无已固定的资产摘要" -DCARET_GTEST_URL=https://invalid.invalid/override)
check_case(local TRUE "" "-DCARET_GTEST_ROOT=${_sdk}" -DGTest_DIR=/nonexistent-system-bypass)
file(MAKE_DIRECTORY "${_root}/missing-config")
file(COPY "${_sdk}/caret-sdk.json" DESTINATION "${_root}/missing-config")
check_case(missing-config FALSE "缺少 GTestConfig.cmake" "-DCARET_GTEST_ROOT=${_root}/missing-config")
check_case(wrong-platform FALSE "版本、平台或 ABI" "-DCARET_GTEST_ROOT=${_sdk}"
           -DCARET_TARGET_TRIPLE=x86_64-unknown-linux-gnu)
set(_archive "${_root}/valid.tar.gz")
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E tar czf "${_archive}" .
  WORKING_DIRECTORY "${_sdk}"
  RESULT_VARIABLE _package_result)
if(NOT _package_result EQUAL 0)
  message(FATAL_ERROR "fixture 归档准备失败")
endif()
file(SHA256 "${_archive}" _sha)
if(WIN32)
  set(_url "file:///${_archive}")
else()
  set(_url "file://${_archive}")
endif()
check_case(wrong-hash FALSE "SHA256 不符" "-DCARET_GTEST_URL=${_url}"
           -DCARET_GTEST_SHA256=0000000000000000000000000000000000000000000000000000000000000000)
check_case(archive TRUE "" "-DCARET_GTEST_URL=${_url}" "-DCARET_GTEST_SHA256=${_sha}")
set(_installed "${_root}/archive-toolchain/sdk/googletest/1.18.0/${_triple}")
file(READ "${_installed}/.caret-archive-sha256" _marker)
string(STRIP "${_marker}" _marker)
if(NOT _marker STREQUAL _sha)
  message(FATAL_ERROR "已安装 SDK 没有正确的已验证摘要标记")
endif()
# 已验证摘要并不代表 CONFIG 契约可用。升级失败必须保留旧包、配置和完成标记。
file(SHA256 "${_installed}/lib/cmake/GTest/GTestConfig.cmake" _good_config_sha)
foreach(_bad_case missing-target wrong-config-version)
  set(_bad_sdk "${_root}/${_bad_case}-sdk")
  file(COPY "${_sdk}/" DESTINATION "${_bad_sdk}")
  if(_bad_case STREQUAL "missing-target")
    file(WRITE "${_bad_sdk}/lib/cmake/GTest/GTestConfig.cmake"
         "add_library(GTest::gtest INTERFACE IMPORTED)\n")
  else()
    file(WRITE "${_bad_sdk}/lib/cmake/GTest/GTestConfigVersion.cmake"
         "set(PACKAGE_VERSION 1.17.0)\nset(PACKAGE_VERSION_COMPATIBLE FALSE)\n")
  endif()
  set(_bad_archive "${_root}/${_bad_case}.tar.gz")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E tar czf "${_bad_archive}" .
    WORKING_DIRECTORY "${_bad_sdk}"
    RESULT_VARIABLE _bad_package_result)
  if(NOT _bad_package_result EQUAL 0)
    message(FATAL_ERROR "坏 CONFIG 归档准备失败")
  endif()
  file(SHA256 "${_bad_archive}" _bad_sha)
  if(WIN32)
    set(_bad_url "file:///${_bad_archive}")
  else()
    set(_bad_url "file://${_bad_archive}")
  endif()
  check_case(archive FALSE "CONFIG 预验失败，保留原安装目录" "-DCARET_GTEST_URL=${_bad_url}"
             "-DCARET_GTEST_SHA256=${_bad_sha}")
  file(READ "${_installed}/.caret-archive-sha256" _retained_marker)
  string(STRIP "${_retained_marker}" _retained_marker)
  file(SHA256 "${_installed}/lib/cmake/GTest/GTestConfig.cmake" _retained_config_sha)
  if(NOT _retained_marker STREQUAL _sha
     OR NOT _retained_config_sha STREQUAL _good_config_sha
     OR EXISTS "${_installed}.previous"
     OR EXISTS "${_installed}.staging")
    message(FATAL_ERROR "${_bad_case} 升级失败没有保留原 SDK 和完成标记")
  endif()
  check_case(archive TRUE "" "-DCARET_GTEST_URL=${_url}" "-DCARET_GTEST_SHA256=${_sha}")
endforeach()

# 来源删除后仍能使用已验证安装树，不重复下载；目录损坏不得静默回退系统包。
file(REMOVE "${_archive}")
check_case(archive TRUE "" "-DCARET_GTEST_URL=${_url}" "-DCARET_GTEST_SHA256=${_sha}")
file(REMOVE "${_installed}/lib/cmake/GTest/GTestConfig.cmake")
check_case(archive FALSE "缺少 GTestConfig.cmake" "-DCARET_GTEST_URL=${_url}"
           "-DCARET_GTEST_SHA256=${_sha}")
message(
  STATUS
    "GoogleTest provision fixtures passed: lazy supply, local CONFIG, pinned archive and rejection paths"
)
