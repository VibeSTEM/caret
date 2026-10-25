# GoogleTest 1.18.0 消费入口。SDK 由 Caret 自己的手动工作流生产，普通构建不编译第三方。
# include 本文件不供给任何资产；真实测试消费者显式调用 caret_require_googletest()。
include_guard(GLOBAL)

function(_caret_gtest_validate_root root triple)
  set(_metadata "${root}/caret-sdk.json")
  if(NOT EXISTS "${_metadata}")
    message(FATAL_ERROR "GoogleTest SDK 缺少 caret-sdk.json：${root}")
  endif()
  file(READ "${_metadata}" _json)
  foreach(
    _field
    name
    version
    triple
    llvm
    cxx_standard
    shared
    windows_shared_crt)
    string(
      JSON
      _value
      ERROR_VARIABLE
      _error
      GET
      "${_json}"
      "${_field}")
    if(NOT _error STREQUAL "NOTFOUND")
      message(FATAL_ERROR "GoogleTest SDK 元数据缺少有效字段 ${_field}：${_error}")
    endif()
    set(_${_field} "${_value}")
  endforeach()
  if(NOT _name STREQUAL "googletest"
     OR NOT _version STREQUAL "1.18.0"
     OR NOT _triple STREQUAL "${triple}"
     OR NOT _llvm STREQUAL "22.1.7"
     OR NOT _cxx_standard STREQUAL "20"
     OR _shared
     OR NOT _windows_shared_crt)
    message(FATAL_ERROR "GoogleTest SDK 版本、平台或 ABI 与基线不符：${_metadata}")
  endif()
  if(NOT EXISTS "${root}/lib/cmake/GTest/GTestConfig.cmake"
     AND NOT EXISTS "${root}/lib64/cmake/GTest/GTestConfig.cmake")
    message(FATAL_ERROR "GoogleTest SDK 缺少 GTestConfig.cmake：${root}")
  endif()
endfunction()

# 以调用方的工具链配置独立探测包，避免污染当前目录的 imported targets。
# 仅在 staging 安装时调用；成功配置后才能写完成标记并替换旧 SDK。
function(_caret_gtest_check_config root result diagnostic)
  set(_probe "${root}.config-check")
  file(REMOVE_RECURSE "${_probe}")
  set(_command "${CMAKE_COMMAND}" -S "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/sdk-config-check" -B
               "${_probe}" -G "${CMAKE_GENERATOR}" "-DCARET_GTEST_CHECK_ROOT=${root}")
  if(CMAKE_GENERATOR_PLATFORM)
    list(APPEND _command -A "${CMAKE_GENERATOR_PLATFORM}")
  endif()
  if(CMAKE_GENERATOR_TOOLSET)
    list(APPEND _command -T "${CMAKE_GENERATOR_TOOLSET}")
  endif()
  foreach(
    _setting IN
    ITEMS CMAKE_TOOLCHAIN_FILE
          CMAKE_MAKE_PROGRAM
          CMAKE_CXX_COMPILER
          CMAKE_CXX_COMPILER_TARGET
          CMAKE_SYSROOT
          CMAKE_OSX_SYSROOT
          CMAKE_OSX_ARCHITECTURES
          CMAKE_OSX_DEPLOYMENT_TARGET
          CMAKE_BUILD_TYPE
          CMAKE_MSVC_RUNTIME_LIBRARY
          CMAKE_MAP_IMPORTED_CONFIG_DEBUG
          CMAKE_CXX_FLAGS
          CMAKE_EXE_LINKER_FLAGS)
    if(DEFINED ${_setting})
      string(REPLACE ";" "\\;" _value "${${_setting}}")
      list(APPEND _command "-D${_setting}:STRING=${_value}")
    endif()
  endforeach()
  execute_process(
    COMMAND ${_command}
    RESULT_VARIABLE _result
    OUTPUT_VARIABLE _stdout
    ERROR_VARIABLE _stderr
    TIMEOUT 120)
  file(REMOVE_RECURSE "${_probe}")
  set(${result}
      "${_result}"
      PARENT_SCOPE)
  set(${diagnostic}
      "${_stdout}\n${_stderr}"
      PARENT_SCOPE)
endfunction()

function(caret_require_googletest)
  # OFF 时不建目录、不找系统包、不读取 URL，也不下载。
  if(NOT BUILD_TESTING)
    return()
  endif()
  # Rust target 与 SDK 资产的平台名分别维护，禁止将宿主包误当成另一 ABI。
  if(CARET_TARGET_TRIPLE STREQUAL "aarch64-apple-darwin")
    set(_sdk_triple "macos-arm64")
  elseif(CARET_TARGET_TRIPLE STREQUAL "x86_64-unknown-linux-gnu")
    set(_sdk_triple "linux-x86_64")
  elseif(CARET_TARGET_TRIPLE STREQUAL "x86_64-pc-windows-msvc")
    set(_sdk_triple "windows-x86_64")
  else()
    message(FATAL_ERROR "GoogleTest SDK 需要已登记的 CARET_TARGET_TRIPLE，实际为 '${CARET_TARGET_TRIPLE}'")
  endif()
  if(CARET_GTEST_ROOT)
    get_filename_component(_root "${CARET_GTEST_ROOT}" ABSOLUTE)
    _caret_gtest_validate_root("${_root}" "${_sdk_triple}")
  else()
    # SHA256 必须固定在调用方配置或本仓库的供给清单，不信任下载源旁的校验文件。
    set(_expected_sha "${CARET_GTEST_SHA256}")
    if(NOT CARET_GTEST_URL AND NOT _expected_sha)
      file(READ "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/googletest-sha256.json" _release_pins)
      string(
        JSON
        _expected_sha
        ERROR_VARIABLE
        _pin_error
        GET
        "${_release_pins}"
        "${_sdk_triple}")
      if(NOT _pin_error STREQUAL "NOTFOUND")
        message(FATAL_ERROR "GoogleTest SDK 供给清单缺少平台摘要：${_sdk_triple}；${_pin_error}")
      endif()
    endif()
    string(LENGTH "${_expected_sha}" _sha_length)
    if(NOT _sha_length EQUAL 64 OR NOT _expected_sha MATCHES "^[A-Fa-f0-9]+$")
      message(
        FATAL_ERROR
          "GoogleTest SDK 尚无已固定的资产摘要；自定义 URL 必须设置 CARET_GTEST_SHA256，或显式设置 CARET_GTEST_ROOT 验证本地 SDK"
      )
    endif()
    string(TOLOWER "${_expected_sha}" _sha)
    if(CARET_GTEST_URL)
      set(_url "${CARET_GTEST_URL}")
    else()
      set(_url
          "https://github.com/VibeSTEM/caret/releases/download/sdk-googletest-1.18.0/googletest-1.18.0-${_sdk_triple}.tar.gz"
      )
    endif()
    if(NOT _url MATCHES "^(https|file)://")
      message(FATAL_ERROR "GoogleTest SDK URL 只接受 HTTPS 或显式本地 file URL：${_url}")
    endif()
    if(CARET_TOOLCHAIN_ROOT)
      get_filename_component(_toolchain "${CARET_TOOLCHAIN_ROOT}" ABSOLUTE)
    elseif(DEFINED ENV{CARET_TOOLCHAIN_ROOT} AND NOT "$ENV{CARET_TOOLCHAIN_ROOT}" STREQUAL "")
      get_filename_component(_toolchain "$ENV{CARET_TOOLCHAIN_ROOT}" ABSOLUTE)
    else()
      get_filename_component(_toolchain "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/../../.toolchain"
                             ABSOLUTE)
    endif()
    set(_root "${_toolchain}/sdk/googletest/1.18.0/${_sdk_triple}")
    file(MAKE_DIRECTORY "${_toolchain}/locks" "${_toolchain}/archives")
    file(
      LOCK "${_toolchain}/locks/googletest-${_sdk_triple}.lock"
      GUARD FUNCTION
      TIMEOUT 120)
    set(_marker "${_root}/.caret-archive-sha256")
    set(_installed_sha "")
    if(EXISTS "${_marker}")
      file(READ "${_marker}" _installed_sha)
      string(STRIP "${_installed_sha}" _installed_sha)
    endif()
    if(NOT _installed_sha STREQUAL _sha)
      set(_archive "${_toolchain}/archives/googletest-${_sdk_triple}-${_sha}.tar.gz")
      file(
        DOWNLOAD "${_url}" "${_archive}"
        STATUS _download
        TLS_VERIFY ON
        TIMEOUT 300)
      list(GET _download 0 _status)
      if(NOT _status EQUAL 0)
        file(REMOVE "${_archive}")
        message(FATAL_ERROR "GoogleTest SDK 下载失败：${_url}；${_download}")
      endif()
      file(SHA256 "${_archive}" _actual_sha)
      if(NOT _actual_sha STREQUAL _sha)
        file(REMOVE "${_archive}")
        message(FATAL_ERROR "GoogleTest SDK SHA256 不符：预期 ${_sha}，实际 ${_actual_sha}")
      endif()
      set(_staging "${_root}.staging")
      file(REMOVE_RECURSE "${_staging}")
      file(MAKE_DIRECTORY "${_staging}")
      execute_process(
        COMMAND "${CMAKE_COMMAND}" -E tar xf "${_archive}"
        WORKING_DIRECTORY "${_staging}"
        RESULT_VARIABLE _extract
        ERROR_VARIABLE _extract_error)
      file(REMOVE "${_archive}")
      if(NOT _extract EQUAL 0)
        file(REMOVE_RECURSE "${_staging}")
        message(FATAL_ERROR "GoogleTest SDK 解包失败：${_extract_error}")
      endif()
      _caret_gtest_validate_root("${_staging}" "${_sdk_triple}")
      _caret_gtest_check_config("${_staging}" _config_result _config_diagnostic)
      if(NOT _config_result STREQUAL "0")
        file(REMOVE_RECURSE "${_staging}")
        message(FATAL_ERROR "GoogleTest SDK CONFIG 预验失败，保留原安装目录：${_config_diagnostic}")
      endif()
      file(WRITE "${_staging}/.caret-archive-sha256" "${_sha}\n")
      set(_backup "${_root}.previous")
      file(REMOVE_RECURSE "${_backup}")
      if(EXISTS "${_root}")
        file(RENAME "${_root}" "${_backup}" RESULT _backup_result)
        if(NOT _backup_result STREQUAL "0")
          message(FATAL_ERROR "GoogleTest SDK 无法暂存旧目录：${_backup_result}")
        endif()
      endif()
      file(RENAME "${_staging}" "${_root}" RESULT _publish_result)
      if(NOT _publish_result STREQUAL "0")
        if(EXISTS "${_backup}")
          file(RENAME "${_backup}" "${_root}")
        endif()
        message(FATAL_ERROR "GoogleTest SDK 发布目录失败：${_publish_result}")
      endif()
      file(REMOVE_RECURSE "${_backup}")
    endif()
    _caret_gtest_validate_root("${_root}" "${_sdk_triple}")
  endif()

  if(TARGET GTest::gtest OR TARGET GTest::gtest_main)
    get_property(_previous_root GLOBAL PROPERTY CARET_GTEST_RESOLVED_ROOT)
    if(NOT _previous_root STREQUAL _root)
      message(FATAL_ERROR "拒绝复用未由 Caret SDK 供给的 GoogleTest targets")
    endif()
    return()
  endif()
  if(EXISTS "${_root}/lib/cmake/GTest/GTestConfig.cmake")
    set(_config "${_root}/lib/cmake/GTest")
  else()
    set(_config "${_root}/lib64/cmake/GTest")
  endif()
  # 清除系统 GTest_DIR 缓存的影响，禁止 PATH 和系统 prefix 搜索旁路。
  set(GTest_DIR "${_config}")
  find_package(
    GTest
    1.18.0
    EXACT
    CONFIG
    REQUIRED
    PATHS
    "${_config}"
    NO_DEFAULT_PATH)
  if(NOT TARGET GTest::gtest OR NOT TARGET GTest::gtest_main)
    message(FATAL_ERROR "GoogleTest SDK CONFIG 未提供 GTest::gtest 和 GTest::gtest_main")
  endif()
  set_property(GLOBAL PROPERTY CARET_GTEST_RESOLVED_ROOT "${_root}")
endfunction()
