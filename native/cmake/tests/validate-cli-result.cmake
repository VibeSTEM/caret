function(caret_validate_invalid_cli_result result stdout stderr valid_out reason_out)
  set(_combined_output "${stdout}\n${stderr}")
  if(_combined_output MATCHES
     "(AddressSanitizer|LeakSanitizer|ThreadSanitizer|UndefinedBehaviorSanitizer|runtime error:)")
    set(${valid_out}
        FALSE
        PARENT_SCOPE)
    set(${reason_out}
        "sanitizer diagnostic detected"
        PARENT_SCOPE)
    return()
  endif()

  if(NOT "${result}" STREQUAL "64")
    set(${valid_out}
        FALSE
        PARENT_SCOPE)
    set(${reason_out}
        "expected exit code 64, got ${result}"
        PARENT_SCOPE)
    return()
  endif()

  if(NOT "${stderr}" MATCHES "--invalid-argument")
    set(${valid_out}
        FALSE
        PARENT_SCOPE)
    set(${reason_out}
        "expected invalid-argument diagnostic was missing"
        PARENT_SCOPE)
    return()
  endif()

  set(${valid_out}
      TRUE
      PARENT_SCOPE)
  set(${reason_out}
      ""
      PARENT_SCOPE)
endfunction()

function(caret_find_sanitizer_log_redirect asan_options lsan_options tsan_options ubsan_options
         var_out)
  foreach(_option_name IN ITEMS ASAN_OPTIONS LSAN_OPTIONS TSAN_OPTIONS UBSAN_OPTIONS)
    if(_option_name STREQUAL "ASAN_OPTIONS")
      set(_options "${asan_options}")
    elseif(_option_name STREQUAL "LSAN_OPTIONS")
      set(_options "${lsan_options}")
    elseif(_option_name STREQUAL "TSAN_OPTIONS")
      set(_options "${tsan_options}")
    else()
      set(_options "${ubsan_options}")
    endif()
    if(_options MATCHES "(^|[:, \t\r\n])log_path[ \t]*=")
      set(${var_out}
          "${_option_name}"
          PARENT_SCOPE)
      return()
    endif()
  endforeach()
  set(${var_out}
      ""
      PARENT_SCOPE)
endfunction()
