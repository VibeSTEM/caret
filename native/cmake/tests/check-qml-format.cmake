# Qml.FormatCheck 驱动（任务 032）：qmlformat（Qt 供给，qmllint 同源）输出
# 与源文件逐字节比对，任何需要重排的文件都列出并失败。qmlformat 本版无
# --check，等价实现为 stdout diff。
#
# 输入：QMLFMT（qmlformat 可执行文件）、QML_DIR（qml/ 源目录）。

if(NOT DEFINED QMLFMT OR NOT DEFINED QML_DIR)
  message(FATAL_ERROR "check-qml-format 需要 -DQMLFMT 与 -DQML_DIR")
endif()
file(GLOB_RECURSE _qml_files "${QML_DIR}/*.qml"
     "${CMAKE_CURRENT_LIST_DIR}/../../../tests/qml/*.qml"
     "${CMAKE_CURRENT_LIST_DIR}/../../../tests/fixtures/qml/*.qml")
if(NOT _qml_files)
  message(FATAL_ERROR "${QML_DIR} 下没有 QML 文件：空套件不能视作通过")
endif()

set(_needs_format "")
foreach(_file IN LISTS _qml_files)
  execute_process(
    COMMAND "${QMLFMT}" "${_file}"
    OUTPUT_VARIABLE _formatted
    ERROR_VARIABLE _err
    RESULT_VARIABLE _rc)
  if(NOT _rc EQUAL 0)
    message(FATAL_ERROR "qmlformat 运行失败（退出码 ${_rc}）：${_file}\n${_err}")
  endif()
  file(READ "${_file}" _content)
  # qmlformat on Windows writes CRLF while repository sources use LF. Compare
  # logical content so the gate checks formatting rather than platform EOLs.
  string(REPLACE "\r\n" "\n" _formatted "${_formatted}")
  string(REPLACE "\r\n" "\n" _content "${_content}")
  if(NOT _formatted STREQUAL _content)
    list(APPEND _needs_format "${_file}")
  endif()
endforeach()

if(_needs_format)
  string(REPLACE ";" "\n  " _listed "${_needs_format}")
  message(FATAL_ERROR "以下 QML 文件需要格式化（qmlformat 后不一致）：\n  ${_listed}\n" "修复：对列出文件运行 qmlformat")
endif()
# 合规时保持静默：汇总输出由 runner 负责，脚本只在发现违规时说话。
