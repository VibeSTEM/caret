if(NOT DEFINED CMAKE_COMMAND OR NOT DEFINED CARET_FIXTURE_ROOT)
  message(FATAL_ERROR "CMAKE_COMMAND 或 CARET_FIXTURE_ROOT 未指定")
endif()

set(_fixture "${CMAKE_CURRENT_LIST_DIR}/layering")
set(_fixture_root "${CARET_FIXTURE_ROOT}")
set(_valid_binary "${_fixture_root}/valid")
set(_invalid_binary "${_fixture_root}/invalid")
set(_prefix_binary "${_fixture_root}/prefix-collision")
set(_alias_binary "${_fixture_root}/hidden-alias")
set(_namespaced_alias_binary "${_fixture_root}/namespaced-alias")
set(_direct_property_binary "${_fixture_root}/direct-property")
set(_computed_genex_binary "${_fixture_root}/computed-genex")
set(_dynamic_property_binary "${_fixture_root}/dynamic-property")
set(_adjacent_genex_binary "${_fixture_root}/adjacent-genex")
set(_nested_concat_binary "${_fixture_root}/nested-concat")
set(_literal_genex_binary "${_fixture_root}/literal-genex-concat")
set(_unregistered_target_binary "${_fixture_root}/unregistered-target")

file(REMOVE_RECURSE "${_fixture_root}")
execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_valid_binary}" -DCARET_FIXTURE_ILLEGAL=OFF
  RESULT_VARIABLE _valid_result
  OUTPUT_VARIABLE _valid_stdout
  ERROR_VARIABLE _valid_stderr)
if(NOT _valid_result EQUAL 0)
  message(FATAL_ERROR "合法分层 fixture 配置失败：\n${_valid_stdout}\n${_valid_stderr}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_invalid_binary}" -DCARET_FIXTURE_ILLEGAL=ON
  RESULT_VARIABLE _invalid_result
  OUTPUT_VARIABLE _invalid_stdout
  ERROR_VARIABLE _invalid_stderr)
if(_invalid_result EQUAL 0)
  message(FATAL_ERROR "违规分层 fixture 意外通过")
endif()

set(_invalid_log "${_invalid_stdout}\n${_invalid_stderr}")
if(NOT _invalid_log MATCHES "caret_shell[ \r\n]*->[ \r\n]*caret_i18n")
  message(FATAL_ERROR "违规 fixture 未报告具体依赖边：\n${_invalid_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_prefix_binary}" -DCARET_FIXTURE_ILLEGAL=OFF
          -DCARET_FIXTURE_PREFIX_COLLISION=ON
  RESULT_VARIABLE _prefix_result
  OUTPUT_VARIABLE _prefix_stdout
  ERROR_VARIABLE _prefix_stderr)
if(_prefix_result EQUAL 0)
  message(FATAL_ERROR "生成表达式中的前缀碰撞依赖意外通过")
endif()

set(_prefix_log "${_prefix_stdout} ${_prefix_stderr}")
if(NOT _prefix_log MATCHES "caret_shell.*caret_shellplugin")
  message(FATAL_ERROR "前缀碰撞 fixture 未报告完整违规 target：${_prefix_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_alias_binary}" -DCARET_FIXTURE_ILLEGAL=OFF
          -DCARET_FIXTURE_HIDDEN_ALIAS=ON
  RESULT_VARIABLE _alias_result
  OUTPUT_VARIABLE _alias_stdout
  ERROR_VARIABLE _alias_stderr)
if(_alias_result EQUAL 0)
  message(FATAL_ERROR "生成表达式中的隐藏 alias 依赖意外通过")
endif()

set(_alias_log "${_alias_stdout} ${_alias_stderr}")
if(NOT _alias_log MATCHES "caret_shell.*caret_native_app")
  message(FATAL_ERROR "隐藏 alias fixture 未报告解析后的 target：${_alias_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_namespaced_alias_binary}"
          -DCARET_FIXTURE_ILLEGAL=OFF -DCARET_FIXTURE_NAMESPACED_ALIAS=ON
  RESULT_VARIABLE _namespaced_alias_result
  OUTPUT_VARIABLE _namespaced_alias_stdout
  ERROR_VARIABLE _namespaced_alias_stderr)
if(_namespaced_alias_result EQUAL 0)
  message(FATAL_ERROR "生成表达式中的命名空间 alias 依赖意外通过")
endif()

set(_namespaced_alias_log "${_namespaced_alias_stdout} ${_namespaced_alias_stderr}")
if(NOT _namespaced_alias_log MATCHES "caret_shell.*caret_native_app")
  message(FATAL_ERROR "命名空间 alias fixture 未报告解析后的 target：${_namespaced_alias_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_direct_property_binary}"
          -DCARET_FIXTURE_ILLEGAL=OFF -DCARET_FIXTURE_DIRECT_PROPERTY=ON
  RESULT_VARIABLE _direct_property_result
  OUTPUT_VARIABLE _direct_property_stdout
  ERROR_VARIABLE _direct_property_stderr)
if(_direct_property_result EQUAL 0)
  message(FATAL_ERROR "INTERFACE_LINK_LIBRARIES_DIRECT 违规依赖意外通过")
endif()

set(_direct_property_log "${_direct_property_stdout} ${_direct_property_stderr}")
if(NOT _direct_property_log MATCHES "caret_shell.*caret_native_app")
  message(FATAL_ERROR "DIRECT 属性 fixture 未报告违规 target：${_direct_property_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_computed_genex_binary}"
          -DCARET_FIXTURE_ILLEGAL=OFF -DCARET_FIXTURE_COMPUTED_GENEX=ON
  RESULT_VARIABLE _computed_genex_result
  OUTPUT_VARIABLE _computed_genex_stdout
  ERROR_VARIABLE _computed_genex_stderr)
if(_computed_genex_result EQUAL 0)
  message(FATAL_ERROR "计算型链接生成表达式意外通过分层检查")
endif()

set(_computed_genex_log "${_computed_genex_stdout} ${_computed_genex_stderr}")
if(NOT _computed_genex_log MATCHES "caret_shell.*JOIN")
  message(FATAL_ERROR "计算型生成表达式 fixture 未报告被拒绝的操作：${_computed_genex_log}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_dynamic_property_binary}"
          -DCARET_FIXTURE_ILLEGAL=OFF -DCARET_FIXTURE_DYNAMIC_PROPERTY=ON
  RESULT_VARIABLE _dynamic_property_result
  OUTPUT_VARIABLE _dynamic_property_stdout
  ERROR_VARIABLE _dynamic_property_stderr)
if(_dynamic_property_result EQUAL 0)
  message(FATAL_ERROR "动态 TARGET_PROPERTY 链接表达式意外通过分层检查")
endif()

set(_dynamic_property_log "${_dynamic_property_stdout} ${_dynamic_property_stderr}")
if(NOT _dynamic_property_log MATCHES "TARGET_PROPERTY")
  message(FATAL_ERROR "动态 TARGET_PROPERTY fixture 未报告拒绝原因：${_dynamic_property_log}")
endif()

foreach(_case IN ITEMS adjacent_genex nested_concat literal_genex_concat namespaced_concat)
  if(_case STREQUAL "adjacent_genex")
    set(_binary "${_adjacent_genex_binary}")
    set(_option CARET_FIXTURE_ADJACENT_GENEX)
  elseif(_case STREQUAL "nested_concat")
    set(_binary "${_nested_concat_binary}")
    set(_option CARET_FIXTURE_NESTED_CONCAT)
  elseif(_case STREQUAL "literal_genex_concat")
    set(_binary "${_literal_genex_binary}")
    set(_option CARET_FIXTURE_LITERAL_GENEX_CONCAT)
  else()
    set(_binary "${_fixture_root}/namespaced-concat")
    set(_option CARET_FIXTURE_NAMESPACED_CONCAT)
  endif()
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_binary}" -DCARET_FIXTURE_ILLEGAL=OFF
            -D${_option}=ON
    RESULT_VARIABLE _result
    OUTPUT_VARIABLE _stdout
    ERROR_VARIABLE _stderr)
  if(_result EQUAL 0)
    message(FATAL_ERROR "${_case} 拼接的链接表达式意外通过分层检查")
  endif()
  set(_log "${_stdout} ${_stderr}")
  if(NOT _log MATCHES "拼接或不完整的链接")
    message(FATAL_ERROR "${_case} fixture 未报告表达式拼接原因：${_log}")
  endif()
endforeach()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${_fixture}" -B "${_unregistered_target_binary}"
          -DCARET_FIXTURE_ILLEGAL=OFF -DCARET_FIXTURE_UNREGISTERED_TARGET=ON
  RESULT_VARIABLE _unregistered_target_result
  OUTPUT_VARIABLE _unregistered_target_stdout
  ERROR_VARIABLE _unregistered_target_stderr)
if(_unregistered_target_result EQUAL 0)
  message(FATAL_ERROR "未登记的仓库 target 意外通过分层检查")
endif()
set(_unregistered_target_log "${_unregistered_target_stdout} ${_unregistered_target_stderr}")
if(NOT _unregistered_target_log MATCHES "项目 native target 未登记.*unregistered_helper")
  message(FATAL_ERROR "未登记 target fixture 未报告具体 target：${_unregistered_target_log}")
endif()

message(STATUS "CMake layering fixtures passed: valid graph accepted; illegal edges diagnosed")
