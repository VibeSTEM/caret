include("${CMAKE_CURRENT_LIST_DIR}/validate-cli-result.cmake")

caret_validate_invalid_cli_result("64" "" "caret: unknown argument '--invalid-argument'" _valid
                                  _reason)
if(NOT _valid)
  message(FATAL_ERROR "expected valid CLI failure to pass validation: ${_reason}")
endif()

caret_validate_invalid_cli_result(
  "64" ""
  "caret: unknown argument '--invalid-argument'\nERROR: LeakSanitizer: detected memory leaks"
  _valid _reason)
if(_valid OR NOT _reason MATCHES "sanitizer diagnostic")
  message(FATAL_ERROR "sanitizer report with exit code 64 was incorrectly accepted")
endif()

caret_validate_invalid_cli_result("1" "" "caret: unknown argument '--invalid-argument'" _valid
                                  _reason)
if(_valid OR NOT _reason MATCHES "exit code 64")
  message(FATAL_ERROR "unexpected CLI exit code was incorrectly accepted")
endif()

caret_find_sanitizer_log_redirect(
  "detect_leaks=1" "suppressions=/tmp/suppressions:log_path=/tmp/lsan" "halt_on_error=1"
  "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "LSAN_OPTIONS log_path redirection was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "verbosity=0 log_path=/tmp/lsan"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "space-separated LSAN_OPTIONS log_path was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "verbosity=0,log_path=/tmp/lsan"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "comma-separated LSAN_OPTIONS log_path was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "verbosity=0\tlog_path=/tmp/lsan"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "tab-separated LSAN_OPTIONS log_path was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "verbosity=0\nlog_path=/tmp/lsan"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "newline-separated LSAN_OPTIONS log_path was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "verbosity=0\rlog_path=/tmp/lsan"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(NOT _redirected_option STREQUAL "LSAN_OPTIONS")
  message(FATAL_ERROR "carriage-return-separated LSAN_OPTIONS log_path was not detected")
endif()

caret_find_sanitizer_log_redirect("detect_leaks=1" "suppressions=/tmp/suppressions"
                                  "halt_on_error=1" "print_stacktrace=1" _redirected_option)
if(_redirected_option)
  message(FATAL_ERROR "normal sanitizer options were incorrectly rejected")
endif()
