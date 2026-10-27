# 在 find_package/FetchContent/qt_add_qml_module 之前包含：Qt 生成的 object
# library 与 GTest 也必须继承 ABI，不能仅修改业务 target。
include_guard(GLOBAL)

get_property(_caret_multi_config GLOBAL PROPERTY GENERATOR_IS_MULTI_CONFIG)
if(NOT _caret_multi_config AND NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE
      Debug
      CACHE STRING "构建类型" FORCE)
endif()
if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX
      "${CMAKE_BINARY_DIR}/install"
      CACHE PATH "安装前缀" FORCE)
endif()
if(NOT CMAKE_EXPORT_COMPILE_COMMANDS)
  set(CMAKE_EXPORT_COMPILE_COMMANDS
      ON
      CACHE BOOL "导出 compile_commands.json" FORCE)
endif()

if(NOT CARET_USE_SYSTEM_TOOLS)
  if(NOT CARET_LLVM_VERSION STREQUAL "22.1.7")
    message(FATAL_ERROR "Cargo 托管 LLVM 版本必须是 22.1.7，实际为 '${CARET_LLVM_VERSION}'")
  endif()
  foreach(_caret_compiler IN ITEMS CMAKE_C_COMPILER CMAKE_CXX_COMPILER)
    execute_process(
      COMMAND "${${_caret_compiler}}" --version
      RESULT_VARIABLE _caret_version_status
      OUTPUT_VARIABLE _caret_version
      ERROR_VARIABLE _caret_version_error
      OUTPUT_STRIP_TRAILING_WHITESPACE)
    if(NOT _caret_version_status EQUAL 0 OR NOT _caret_version MATCHES "clang version 22\\.1\\.7")
      message(
        FATAL_ERROR "${_caret_compiler} 未使用 LLVM 22.1.7：${_caret_version} ${_caret_version_error}")
    endif()
  endforeach()
endif()

# 编译器统一为 LLVM，标准库头与运行库仍跟随平台 SDK，避免新版 libc++
# 头引用当前 macOS 系统运行库尚未提供的符号（如 __hash_memory）。
if(APPLE AND NOT CARET_USE_SYSTEM_TOOLS)
  if(NOT EXISTS "${CMAKE_OSX_SYSROOT}/usr/include/c++/v1/string")
    message(FATAL_ERROR "Apple SDK 缺少 libc++ 头文件，请通过 Cargo 配置 sysroot")
  endif()
  add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:-nostdinc++>"
                      "$<$<COMPILE_LANGUAGE:CXX>:-isystem${CMAKE_OSX_SYSROOT}/usr/include/c++/v1>")
endif()

if(CARET_ENABLE_COVERAGE)
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    message(FATAL_ERROR "CARET_ENABLE_COVERAGE 需要 Clang C++ 编译器")
  endif()
  add_compile_options(-fprofile-instr-generate -fcoverage-mapping)
  add_link_options(-fprofile-instr-generate)
endif()

# sanitizer 矩阵：值为逗号分隔的 address/undefined/thread 组合，
# 空关闭。ASan+UBSan 是三平台主组合；TSan 与其他 sanitizer 运行库互斥，
# 只能独立构建。三平台不等价：TSan 官方支持平台不含 Windows，clang-cl 仅
# 支持部分 UBSan 检查，Windows 矩阵只验证 ASan；不满足的组合在 configure
# 期失败，不隐式降级。
set(CARET_SANITIZER
    ""
    CACHE STRING "逗号分隔的 sanitizer：address、undefined、thread；空关闭")
string(REPLACE "," ";" _caret_sanitizers "${CARET_SANITIZER}")
list(REMOVE_ITEM _caret_sanitizers "")
if(_caret_sanitizers)
  if(CARET_ENABLE_COVERAGE)
    message(FATAL_ERROR "CARET_SANITIZER 与 CARET_ENABLE_COVERAGE 不组合验证")
  endif()
  foreach(_caret_sanitizer IN LISTS _caret_sanitizers)
    if(NOT _caret_sanitizer MATCHES "^(address|undefined|thread)$")
      message(FATAL_ERROR "未知 sanitizer '${_caret_sanitizer}'；支持 address、undefined、thread")
    endif()
  endforeach()
  list(FIND _caret_sanitizers "thread" _caret_tsan_index)
  list(FIND _caret_sanitizers "undefined" _caret_ubsan_index)
  if(NOT _caret_tsan_index EQUAL -1)
    if(NOT _caret_sanitizers STREQUAL "thread")
      message(FATAL_ERROR "TSan 与其他 sanitizer 运行库互斥，只能单独启用：${CARET_SANITIZER}")
    endif()
    if(MSVC)
      message(FATAL_ERROR "TSan 官方支持平台不含 Windows，见 clang ThreadSanitizer 支持平台列表")
    endif()
  endif()
  if(MSVC AND NOT _caret_ubsan_index EQUAL -1)
    message(FATAL_ERROR "clang-cl 仅支持部分 UBSan 检查，Windows 矩阵只验证 ASan")
  endif()
  string(REPLACE ";" "," _caret_sanitizer_flags "${_caret_sanitizers}")
  add_compile_options(-fno-omit-frame-pointer -fsanitize=${_caret_sanitizer_flags})
  if(NOT _caret_ubsan_index EQUAL -1)
    # UBSan 缺省只报告不失败；测试门禁要求错误即非零退出。
    add_compile_options(-fno-sanitize-recover=undefined)
  endif()
  if(MSVC)
    # CMake 对 clang-cl 直接以 lld-link 链接，驱动器不会注入 compiler-rt，
    # 按其 /MD 下的注入序列显式复刻；运行期 DLL 由测试环境把编译器资源
    # 目录加入 PATH 解析（caret-build::compiler_rt_dll_dir）。
    execute_process(
      COMMAND "${CMAKE_CXX_COMPILER}" "-print-resource-dir"
      OUTPUT_VARIABLE _caret_asan_resource
      RESULT_VARIABLE _caret_asan_resource_result
      OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
    if(NOT _caret_asan_resource_result EQUAL 0 OR NOT IS_DIRECTORY "${_caret_asan_resource}")
      message(FATAL_ERROR "clang-cl -print-resource-dir 失败，无法定位 Windows ASan "
                          "运行库：${CMAKE_CXX_COMPILER}")
    endif()
    if(NOT CMAKE_SYSTEM_PROCESSOR MATCHES "^(AMD64|x86_64)$")
      message(FATAL_ERROR "Windows ASan 矩阵只登记 x64，未知架构 " "${CMAKE_SYSTEM_PROCESSOR}")
    endif()
    # lld-link 的响应文件词法把 `\x` 当转义消费，路径统一正斜杠并加引号。
    string(REPLACE "\\" "/" _caret_asan_rt_dir "${_caret_asan_resource}/lib/windows")
    foreach(_caret_asan_rt_lib asan_dynamic asan_dynamic_runtime_thunk)
      if(NOT EXISTS "${_caret_asan_rt_dir}/clang_rt.${_caret_asan_rt_lib}-x86_64.lib")
        message(
          FATAL_ERROR "LLVM 发行包缺少 compiler-rt 运行库 " "clang_rt.${_caret_asan_rt_lib}-x86_64.lib"
                      "（${_caret_asan_rt_dir}）；lib/clang 资源目录不得裁剪")
      endif()
    endforeach()
    # STL 容器注解经 detect_mismatch 固化进目标文件，与未插桩的 CXX 侧
    # C++（caret_ffi）混链即 failifmismatch；按 STL 官方混链场景开关统一
    # 关闭，Windows 矩阵失去容器溢出检测。
    add_compile_definitions(_DISABLE_STL_ANNOTATION)
    add_link_options("-debug" "-incremental:no")
    # Windows-MSVC 链接规则把 LINK_FLAGS 放在对象文件之后。把 ASan 导入库前置到
    # 各链接规则的对象输入之前，确保进程启动时先加载 ASan，再加载 Qt 等依赖。
    set(_caret_asan_import_lib "\"${_caret_asan_rt_dir}/clang_rt.asan_dynamic-x86_64.lib\"")
    foreach(_caret_link_rule IN ITEMS
            CMAKE_C_LINK_EXECUTABLE CMAKE_CXX_LINK_EXECUTABLE
            CMAKE_C_CREATE_SHARED_LIBRARY CMAKE_CXX_CREATE_SHARED_LIBRARY
            CMAKE_C_CREATE_SHARED_MODULE CMAKE_CXX_CREATE_SHARED_MODULE)
      if(DEFINED ${_caret_link_rule})
        string(REPLACE "<OBJECTS>" "${_caret_asan_import_lib} <OBJECTS>"
                       _caret_link_command "${${_caret_link_rule}}")
        if(_caret_link_command STREQUAL "${${_caret_link_rule}}")
          message(FATAL_ERROR "Windows ASan 无法将运行库前置到链接规则 ${_caret_link_rule}")
        endif()
        set(${_caret_link_rule} "${_caret_link_command}")
      endif()
    endforeach()
    add_link_options("-include:__asan_seh_interceptor")
    add_link_options(
      "SHELL:-wholearchive:\"${_caret_asan_rt_dir}/clang_rt.asan_dynamic_runtime_thunk-x86_64.lib\""
    )
  endif()
  # 运行期检查 PE 导入表，确保 ASan 是首个导入依赖。
  add_link_options(-fsanitize=${_caret_sanitizer_flags})
endif()

if(MSVC)
  # 当前 Rust/CXX staticlib 使用 /MD；Debug 保留调试符号及未优化代码，
  # 但整个 native 图统一使用 release CRT/STL ABI，包括 Qt 生成目标。
  set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
  add_compile_definitions(_ITERATOR_DEBUG_LEVEL=0)
  # Qt 也选择 release ABI，避免 /MD 对象链接 Qt Debug DLL 导致混用 CRT。
  # 空元素允许无配置 imported 工具；不回退到 Debug 库。
  set(CMAKE_MAP_IMPORTED_CONFIG_DEBUG "Release;RelWithDebInfo;")
endif()

# 告警仅应用到自有代码；生成代码和第三方只继承上面的 ABI 策略。
function(caret_native_defaults target)
  target_compile_features(${target} PUBLIC cxx_std_20)
  set_target_properties(${target} PROPERTIES CXX_EXTENSIONS OFF)
  if(MSVC)
    target_compile_options(${target} PRIVATE /W4 /permissive-)
    # Rust/CXX bridge 的错误边界抛出 rust::Error；clang-cl 默认关闭异常。
    target_compile_options(${target} PRIVATE /EHsc)
  else()
    target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic)
  endif()
endfunction()

# QML 测试统一使用托管 Qt、无窗口平台与可定制的 Controls 样式。
# Windows 无控制台时 QtTest 默认写调试输出；强制 stderr 使 CTest 能展示断言。
# 普通 QML 测试超时为 20s；sanitizer 下放宽到 120s，防止工具或平台
# 挂起拖垮整个 CI。实际超时和失败证据记录在 Caret 的对应任务中。
function(caret_add_qml_test name target)
  set(_caret_qml_timeout 20)
  if(_caret_sanitizers)
    set(_caret_qml_timeout 120)
  endif()
  set(_caret_qml_labels qml)
  if(ARGC GREATER 2)
    list(APPEND _caret_qml_labels "${ARGV2}")
  endif()
  add_test(NAME ${name} COMMAND ${target})
  set_tests_properties(
    ${name}
    PROPERTIES
      TIMEOUT
      ${_caret_qml_timeout}
      LABELS
      "${_caret_qml_labels}"
      ENVIRONMENT
      "QT_QPA_PLATFORM=offscreen;QT_FORCE_STDERR_LOGGING=1;QT_QUICK_CONTROLS_STYLE=Basic;QT_PLUGIN_PATH=${QT_STAGING}/plugins;QML_IMPORT_PATH=${QT_STAGING}/qml"
  )
endfunction()

# 递归核对真实构建图（包括 Qt 生成的 object libraries），提前阻断上游或
# 子目录覆盖运行库策略。依赖自己的源文件也必须遵守相同 ABI。
function(caret_verify_msvc_abi directory)
  if(NOT MSVC)
    return()
  endif()
  get_property(
    _targets
    DIRECTORY "${directory}"
    PROPERTY BUILDSYSTEM_TARGETS)
  get_property(
    _directory_defines
    DIRECTORY "${directory}"
    PROPERTY COMPILE_DEFINITIONS)
  foreach(_target IN LISTS _targets)
    get_target_property(_type ${_target} TYPE)
    if(_type MATCHES "^(EXECUTABLE|STATIC_LIBRARY|SHARED_LIBRARY|MODULE_LIBRARY|OBJECT_LIBRARY)$")
      get_target_property(_runtime ${_target} MSVC_RUNTIME_LIBRARY)
      get_property(
        _defines
        TARGET ${_target}
        PROPERTY COMPILE_DEFINITIONS)
      list(APPEND _defines ${_directory_defines})
      if(NOT _runtime STREQUAL "MultiThreadedDLL"
         OR NOT "_ITERATOR_DEBUG_LEVEL=0" IN_LIST _defines
         OR "_ITERATOR_DEBUG_LEVEL=1" IN_LIST _defines
         OR "_ITERATOR_DEBUG_LEVEL=2" IN_LIST _defines)
        message(
          FATAL_ERROR
            "${_target}: inconsistent MSVC ABI (${_runtime}; ${_defines}); expected /MD and _ITERATOR_DEBUG_LEVEL=0"
        )
      endif()
    endif()
  endforeach()
  get_property(
    _directories
    DIRECTORY "${directory}"
    PROPERTY SUBDIRECTORIES)
  foreach(_directory IN LISTS _directories)
    caret_verify_msvc_abi("${_directory}")
  endforeach()
endfunction()
