cmake_minimum_required(VERSION 3.22)
project(caret_layering_fixture LANGUAGES CXX)

set(CARET_ENABLE_APP ON)
set(CARET_FFI_STATIC_LIB "${CMAKE_BINARY_DIR}/libcaret_ffi.a")
include("${CMAKE_CURRENT_LIST_DIR}/../../layering.cmake")

foreach(_target IN ITEMS i18n shell shellplugin adapters adaptersplugin native_app)
  add_library("caret_${_target}" INTERFACE)
endforeach()

foreach(_target IN ITEMS Experimental NightlyMemoryCheck all_qmltyperegistrations all_qmllint
                         all_qmllint_json all_qmllint_module)
  add_custom_target("${_target}" COMMENT "模拟 Qt/CTest 生成的 utility target")
endforeach()

add_library(caret::i18n ALIAS caret_i18n)
add_library(caret::shell ALIAS caret_shell)
add_library(caret::adapters ALIAS caret_adapters)
target_link_libraries(caret_shellplugin INTERFACE caret::shell)
target_link_libraries(caret_shellplugin INTERFACE "$<LINK_ONLY:caret::shell>")
target_link_libraries(caret_adaptersplugin INTERFACE caret_adapters)
target_link_libraries(caret_shell INTERFACE caret::adapters)
target_link_libraries(caret_shellplugin INTERFACE caret_adaptersplugin)
target_link_libraries(caret_native_app INTERFACE caret::shell caret_shellplugin caret::adapters
                                                 caret_adaptersplugin caret::i18n
                                                 "${CARET_FFI_STATIC_LIB}")

if(CARET_FIXTURE_ILLEGAL)
  target_link_libraries(caret_shell INTERFACE caret_i18n)
endif()

if(CARET_FIXTURE_UNREGISTERED_TARGET)
  add_library(unregistered_helper INTERFACE)
  target_link_libraries(caret_shell INTERFACE unregistered_helper)
endif()

if(CARET_FIXTURE_PREFIX_COLLISION)
  target_link_libraries(caret_shellplugin INTERFACE "$<$<BOOL:1>:caret_shellplugin>")
endif()

if(CARET_FIXTURE_HIDDEN_ALIAS)
  add_library(hidden_alias ALIAS caret_native_app)
  target_link_libraries(caret_shell INTERFACE "$<LINK_ONLY:hidden_alias>")
endif()

if(CARET_FIXTURE_NAMESPACED_ALIAS)
  add_library(hidden::app ALIAS caret_native_app)
  target_link_libraries(caret_shell INTERFACE "$<LINK_ONLY:hidden::app>")
endif()

if(CARET_FIXTURE_DIRECT_PROPERTY)
  set_property(TARGET caret_shell PROPERTY INTERFACE_LINK_LIBRARIES_DIRECT caret_native_app)
endif()

if(CARET_FIXTURE_COMPUTED_GENEX)
  target_link_libraries(caret_shell INTERFACE "$<JOIN:caret_;native_app,>")
endif()

if(CARET_FIXTURE_DYNAMIC_PROPERTY)
  target_link_libraries(caret_shell INTERFACE "$<TARGET_PROPERTY:INTERFACE_LINK_LIBRARIES>")
endif()

if(CARET_FIXTURE_ADJACENT_GENEX)
  target_link_libraries(caret_shell INTERFACE "$<1:caret_>$<1:native_app>")
endif()

if(CARET_FIXTURE_NESTED_CONCAT)
  target_link_libraries(caret_shell INTERFACE "$<1:caret_$<1:native_app>>")
endif()

if(CARET_FIXTURE_LITERAL_GENEX_CONCAT)
  add_library(hidden_app ALIAS caret_native_app)
  target_link_libraries(caret_shell INTERFACE "hidden_$<1:native_app>")
endif()

if(CARET_FIXTURE_NAMESPACED_CONCAT)
  add_library(hidden::app ALIAS caret_native_app)
  target_link_libraries(caret_shell INTERFACE "$<LINK_ONLY:hidden::$<1:app>>")
endif()

cmake_language(DEFER CALL caret_verify_layering)
