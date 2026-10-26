# 来源 panta ee84d95；保留表达式与 alias 校验，允许边按 Caret 当前目标裁剪。
# Rust/CMake 分层检查依赖实际配置后的 target link 属性；不从源码文本猜测边。
function(_caret_collect_directory_targets directory output)
  get_property(
    _directory_targets
    DIRECTORY "${directory}"
    PROPERTY BUILDSYSTEM_TARGETS)
  get_property(
    _subdirectories
    DIRECTORY "${directory}"
    PROPERTY SUBDIRECTORIES)
  foreach(_subdirectory IN LISTS _subdirectories)
    _caret_collect_directory_targets("${_subdirectory}" _child_targets)
    list(APPEND _directory_targets ${_child_targets})
  endforeach()
  set("${output}"
      "${_directory_targets}"
      PARENT_SCOPE)
endfunction()

# 验证生成表达式的输出分支可静态归约到完整依赖项，拒绝字面量或多个
# 生成表达式拼接 target 名。Qt 生成的 TARGET_OBJECTS/TARGET_NAME 与
# LINK_ONLY/条件包装按结构递归处理。
function(_caret_validate_link_output source expression link output_operator output_value)
  if(NOT expression MATCHES "\\$<")
    set("${output_operator}"
        ""
        PARENT_SCOPE)
    set("${output_value}"
        "${expression}"
        PARENT_SCOPE)
    return()
  endif()
  if(NOT expression MATCHES "^\\$<")
    message(FATAL_ERROR "分层检查拒绝拼接或不完整的链接输出：${source} -> ${link}")
  endif()

  string(LENGTH "${expression}" _length)
  set(_depth 0)
  set(_separator -1)
  set(_outer_close -1)
  set(_index 0)
  while(_index LESS _length)
    math(EXPR _remaining "${_length} - ${_index}")
    if(_remaining GREATER 1)
      string(SUBSTRING "${expression}" ${_index} 2 _pair)
    else()
      set(_pair "")
    endif()
    if(_pair STREQUAL "$<")
      math(EXPR _depth "${_depth} + 1")
      math(EXPR _index "${_index} + 2")
      continue()
    endif()
    string(SUBSTRING "${expression}" ${_index} 1 _character)
    if(_character STREQUAL ">")
      math(EXPR _depth "${_depth} - 1")
      if(_depth EQUAL 0)
        set(_outer_close "${_index}")
        break()
      endif()
    elseif(
      _character STREQUAL ":"
      AND _depth EQUAL 1
      AND _separator LESS 0)
      set(_separator "${_index}")
    endif()
    math(EXPR _index "${_index} + 1")
  endwhile()
  math(EXPR _last_index "${_length} - 1")
  if(NOT _outer_close EQUAL _last_index OR _separator LESS 0)
    message(FATAL_ERROR "分层检查拒绝拼接或不完整的链接输出：${source} -> ${link}")
  endif()

  if(expression MATCHES "^\\$<([A-Za-z_][A-Za-z0-9_]*)[:>]")
    set(_operator "${CMAKE_MATCH_1}")
  else()
    set(_operator "")
  endif()
  math(EXPR _value_start "${_separator} + 1")
  math(EXPR _value_length "${_length} - ${_value_start} - 1")
  string(SUBSTRING "${expression}" ${_value_start} ${_value_length} _value)

  if(_operator STREQUAL "TARGET_NAME")
    if(_value MATCHES "\\$<" OR _value MATCHES "[,;]")
      message(FATAL_ERROR "分层检查拒绝动态 TARGET_NAME 输出：${source} -> ${link}")
    endif()
  elseif(
    _operator STREQUAL "LINK_ONLY"
    OR _operator STREQUAL "BUILD_INTERFACE"
    OR _operator STREQUAL "INSTALL_INTERFACE"
    OR _operator STREQUAL "TARGET_OBJECTS"
    OR _operator STREQUAL "")
    _caret_validate_link_output("${source}" "${_value}" "${link}" _nested_operator _nested_value)
  else()
    message(FATAL_ERROR "分层检查不支持链接输出表达式 ${_operator}：${source} -> ${link}")
  endif()
  set("${output_operator}"
      "${_operator}"
      PARENT_SCOPE)
  set("${output_value}"
      "${_value}"
      PARENT_SCOPE)
endfunction()

function(caret_verify_layering)
  set(_production_targets caret_shell caret_shellplugin caret_adapters caret_adaptersplugin)
  if(TARGET caret_canvas)
    list(APPEND _production_targets caret_canvas)
  endif()
  if(TARGET caret_canvasplugin)
    list(APPEND _production_targets caret_canvasplugin)
  endif()
  if(TARGET caret_viewmodels)
    list(APPEND _production_targets caret_viewmodels)
  endif()
  if(TARGET caret_viewmodelsplugin)
    list(APPEND _production_targets caret_viewmodelsplugin)
  endif()
  if(TARGET caret_i18n)
    list(APPEND _production_targets caret_i18n)
  endif()
  if(CARET_ENABLE_APP)
    list(APPEND _production_targets caret_native_app)
  endif()

  _caret_collect_directory_targets("${CMAKE_SOURCE_DIR}" _configured_targets)
  foreach(_target IN LISTS _configured_targets)
    get_target_property(_target_source_dir "${_target}" SOURCE_DIR)
    if(NOT _target_source_dir)
      continue()
    endif()
    get_filename_component(_repository_root "${CMAKE_SOURCE_DIR}/.." ABSOLUTE)
    cmake_path(IS_PREFIX _repository_root "${_target_source_dir}" _project_owned_target NORMALIZE)
    if(NOT _project_owned_target OR _target IN_LIST _production_targets)
      continue()
    endif()
    get_target_property(_target_type "${_target}" TYPE)
    # CTest、Qt 等会在调用方目录创建聚合 custom target；它们不能作为链接库
    # 绕过依赖图检查，只审计可参与链接图的仓库自有 targets。
    if(_target_type STREQUAL "UTILITY")
      continue()
    endif()
    if(_target MATCHES "(_test|_benchmark)(_other_files)?$"
       OR _target STREQUAL "caret_performance_fixture"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)(_resources_[0-9]+|_tooling)$"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)plugin_init$"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)_other_files$"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)_qml[a-zA-Z0-9_]*$"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)_.*_qml$"
       OR _target MATCHES "^caret_(shell|canvas|viewmodels|adapters)_copy_(qml|res)$"
       OR _target MATCHES
          "^caret_native_app_(qml[a-zA-Z0-9_]*|.*_qml|copy_(qml|res)|other_files|tooling)$"
       OR _target MATCHES "^(caret_native_app_tooling|caret_benchmark_moc)$")
      continue()
    endif()
    message(FATAL_ERROR "项目 native target 未登记分层（须使用已登记的 caret_ target 名）：${_target}")
  endforeach()

  foreach(_target IN LISTS _production_targets)
    if(NOT TARGET "${_target}")
      message(FATAL_ERROR "分层检查无法解析项目 target '${_target}'")
    endif()
  endforeach()

  foreach(_source IN LISTS _production_targets)

    set(_supported_link_genex
        LINK_ONLY
        BUILD_INTERFACE
        INSTALL_INTERFACE
        BOOL
        IF
        CONFIG
        AND
        NOT
        STREQUAL
        TARGET_OBJECTS
        TARGET_NAME)
    set(_allowed_dependencies)
    set(_allowed_aliases)
    if(_source STREQUAL "caret_shell")
      set(_allowed_dependencies caret_canvas caret_viewmodels caret_adapters caret_adaptersplugin)
      set(_allowed_aliases caret::canvas caret::viewmodels caret::adapters)
    elseif(_source STREQUAL "caret_shellplugin")
      # Qt 为 Shell 使用的静态 QML 适配模块生成传递插件依赖。
      set(_allowed_dependencies caret_shell caret_adaptersplugin)
      set(_allowed_aliases caret::shell)
    elseif(_source STREQUAL "caret_adaptersplugin")
      set(_allowed_dependencies caret_adapters)
    elseif(_source STREQUAL "caret_canvasplugin")
      set(_allowed_dependencies caret_canvas)
    elseif(_source STREQUAL "caret_viewmodelsplugin")
      set(_allowed_dependencies caret_viewmodels)
    elseif(_source STREQUAL "caret_viewmodels")
      set(_allowed_dependencies caret_ffi)
    elseif(_source STREQUAL "caret_native_app")
      set(_allowed_dependencies
          caret_adapters
          caret_adaptersplugin
          caret_shell
          caret_shellplugin
          caret_canvas
          caret_canvasplugin
          caret_viewmodels
          caret_viewmodelsplugin
          caret_i18n
          caret_ffi)
      set(_allowed_aliases caret::adapters caret::shell caret::canvas caret::viewmodels caret::i18n)
    endif()

    set(_links)
    foreach(_property IN ITEMS LINK_LIBRARIES INTERFACE_LINK_LIBRARIES
                               INTERFACE_LINK_LIBRARIES_DIRECT)
      get_target_property(_property_links "${_source}" "${_property}")
      if(_property_links AND NOT _property_links MATCHES "-NOTFOUND$")
        list(APPEND _links ${_property_links})
      endif()
    endforeach()
    list(REMOVE_DUPLICATES _links)

    foreach(_link IN LISTS _links)
      if(_link MATCHES "\\$<")
        # 表达式可能藏在字面量后缀/前缀中；先验证所有含生成表达式的项，
        # 再按完整表达式处理，避免把合成库名当作外部依赖忽略。
        _caret_validate_link_output("${_source}" "${_link}" "${_link}" _root_operator _link_value)
      endif()
      # 条件链接表达式不能安全地按字符串拆解；分层图中的仓库内边必须显式
      # 可解析。只接受结构已知的表达式，防止 JOIN/TARGET_PROPERTY 等计算
      # 型表达式拼出静态 token 扫描看不到的项目 target。
      if(_link MATCHES "^\\$<")
        if(_root_operator)
          if(NOT _root_operator IN_LIST _supported_link_genex)
            message(FATAL_ERROR "分层检查不支持链接生成表达式：${_source} -> ${_link}（${_root_operator}）")
          endif()
        endif()

        string(REGEX MATCHALL "\\$<[A-Za-z_][A-Za-z0-9_]*[:>]" _genex_operators "${_link_value}")
        foreach(_expression IN LISTS _genex_operators)
          string(REGEX REPLACE "^\\$<|[:>]$" "" _operator "${_expression}")
          if(NOT _operator IN_LIST _supported_link_genex)
            message(FATAL_ERROR "分层检查不支持无法静态验证的链接生成表达式：${_source} -> ${_link}（${_operator}）")
          endif()
        endforeach()

        string(REGEX MATCHALL "caret(::|_)[A-Za-z0-9_.+-]+" _project_tokens "${_link}")
        foreach(_dependency IN LISTS _project_tokens)
          if(_dependency IN_LIST _allowed_dependencies OR _dependency IN_LIST _allowed_aliases)
            continue()
          endif()
          if(_dependency
             MATCHES
             "^caret_(shell|canvas|viewmodels|adapters)(_resources_[0-9]+|_tooling|_other_files|_qml[a-zA-Z0-9_]*|_.*_qml|_copy_(qml|res)|plugin_init)$"
             OR _dependency MATCHES
                "^caret_native_app_(qml[a-zA-Z0-9_]*|.*_qml|copy_(qml|res)|other_files|tooling)$")
            continue()
          endif()
          message(FATAL_ERROR "条件链接表达式含未登记的项目 target：${_source} -> ${_dependency}（表达式：${_link}）")
        endforeach()

        # 非 caret 命名的 ALIAS 也可能指向项目 target。检查表达式中的标识符，
        # 并将 CMake alias 解析到真实 target 后按实际依赖边校验。
        string(REGEX MATCHALL "[A-Za-z_][A-Za-z0-9_.+-]*(::[A-Za-z_][A-Za-z0-9_.+-]*)*"
                     _expression_tokens "${_link}")
        foreach(_token IN LISTS _expression_tokens)
          if(NOT TARGET "${_token}")
            continue()
          endif()
          get_target_property(_resolved_target "${_token}" ALIASED_TARGET)
          if(NOT _resolved_target)
            set(_resolved_target "${_token}")
          endif()
          if(NOT _resolved_target MATCHES "^caret_")
            continue()
          endif()
          if(_resolved_target IN_LIST _allowed_dependencies
             OR _resolved_target
                MATCHES
                "^caret_(shell|canvas|viewmodels|adapters)(_resources_[0-9]+|_tooling|_other_files|_qml[a-zA-Z0-9_]*|_.*_qml|_copy_(qml|res)|plugin_init)$"
             OR _resolved_target MATCHES
                "^caret_native_app_(qml[a-zA-Z0-9_]*|.*_qml|copy_(qml|res)|other_files|tooling)$")
            continue()
          endif()
          message(
            FATAL_ERROR "条件链接表达式经 alias 解析后违反分层规则：${_source} -> ${_resolved_target}（表达式：${_link}）")
        endforeach()
        continue()
      endif()

      if("${_link}" STREQUAL "${CARET_FFI_STATIC_LIB}")
        set(_dependency caret_ffi)
      elseif(TARGET "${_link}")
        get_target_property(_dependency "${_link}" ALIASED_TARGET)
        if(NOT _dependency)
          set(_dependency "${_link}")
        endif()
      elseif(_link MATCHES "^caret(::|_)")
        message(FATAL_ERROR "分层检查无法解析项目 target：${_source} -> ${_link}")
      else()
        continue()
      endif()

      if(NOT _dependency MATCHES "^caret_")
        continue()
      endif()

      if(_dependency MATCHES
         "^caret_(shell|canvas|viewmodels|adapters)(_resources_[0-9]+|_tooling|plugin_init)$"
         OR _dependency MATCHES "^caret_(shell|canvas|viewmodels|adapters)_other_files$"
         OR _dependency MATCHES "^caret_(shell|canvas|viewmodels|adapters)_qml[a-zA-Z0-9_]*$"
         OR _dependency MATCHES "^caret_(shell|canvas|viewmodels|adapters)_.*_qml$"
         OR _dependency MATCHES "^caret_(shell|canvas|viewmodels|adapters)_copy_(qml|res)$"
         OR _dependency MATCHES
            "^caret_native_app_(qml[a-zA-Z0-9_]*|.*_qml|copy_(qml|res)|other_files|tooling)$"
         OR _dependency STREQUAL "caret_native_app_tooling")
        continue()
      endif()

      if(NOT _dependency IN_LIST _allowed_dependencies)
        message(FATAL_ERROR "违反 Rust/CMake 分层规则：${_source} -> ${_dependency}（未登记允许边）")
      endif()
    endforeach()
  endforeach()
endfunction()
