if(NOT DEFINED CARET_APP_EXECUTABLE OR NOT EXISTS "${CARET_APP_EXECUTABLE}")
  message(FATAL_ERROR "缺少可执行的 Caret 应用：${CARET_APP_EXECUTABLE}")
endif()

include("${CMAKE_CURRENT_LIST_DIR}/validate-cli-result.cmake")

caret_find_sanitizer_log_redirect("$ENV{ASAN_OPTIONS}" "$ENV{LSAN_OPTIONS}" "$ENV{TSAN_OPTIONS}"
                                  "$ENV{UBSAN_OPTIONS}" _caret_redirected_sanitizer)
if(_caret_redirected_sanitizer)
  message(FATAL_ERROR "${_caret_redirected_sanitizer} 使用了 log_path；该测试要求 sanitizer 报告输出到 stderr")
endif()

execute_process(
  COMMAND "${CARET_APP_EXECUTABLE}" --invalid-argument
  RESULT_VARIABLE _caret_result
  OUTPUT_VARIABLE _caret_stdout
  ERROR_VARIABLE _caret_stderr)

caret_validate_invalid_cli_result("${_caret_result}" "${_caret_stdout}" "${_caret_stderr}"
                                  _caret_valid _caret_reason)
if(NOT _caret_valid)
  message(
    FATAL_ERROR
      "无效 CLI 参数检查失败：${_caret_reason}\nstdout:\n${_caret_stdout}\nstderr:\n${_caret_stderr}")
endif()
