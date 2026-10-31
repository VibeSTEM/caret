if(NOT DEFINED CARET_APP)
  message(FATAL_ERROR "CARET_APP must name the built Caret executable")
endif()

execute_process(
  COMMAND
    "${CMAKE_COMMAND}" -E env "QT_QPA_PLATFORM=offscreen"
    "HOME=${CMAKE_CURRENT_BINARY_DIR}/test-home"
    "XDG_CONFIG_HOME=${CMAKE_CURRENT_BINARY_DIR}/test-config"
    "XDG_DATA_HOME=${CMAKE_CURRENT_BINARY_DIR}/test-data"
    "APPDATA=${CMAKE_CURRENT_BINARY_DIR}/test-appdata"
    "LOCALAPPDATA=${CMAKE_CURRENT_BINARY_DIR}/test-localappdata" "${CARET_APP}" --quit-after-load --
    -platform caret-platform-argument-probe
  RESULT_VARIABLE result
  OUTPUT_VARIABLE stdout
  ERROR_VARIABLE stderr)

if("${result}" STREQUAL "0")
  message(FATAL_ERROR "Caret ignored the Qt arguments after --: ${stdout}${stderr}")
endif()
if(NOT "${stderr}" MATCHES "Could not find the Qt platform plugin.*caret-platform-argument-probe")
  message(FATAL_ERROR "Caret failed for an unexpected reason (${result}): ${stdout}${stderr}")
endif()
