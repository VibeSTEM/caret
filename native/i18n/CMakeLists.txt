# 消费构建树里的 TS，用锁定 lrelease 编译 QM。TS 与 QM 都不入库。

if(NOT EXISTS "${CARET_I18N_TS_DIR}")
  message(FATAL_ERROR "CARET_I18N_TS_DIR 指向的目录不存在：${CARET_I18N_TS_DIR}")
endif()

find_program(
  CARET_LRELEASE_EXECUTABLE
  NAMES lrelease
  PATHS "${QT_STAGING}/bin"
  NO_DEFAULT_PATH REQUIRED)
execute_process(
  COMMAND "${CARET_LRELEASE_EXECUTABLE}" -version
  OUTPUT_VARIABLE _lrelease_version
  ERROR_VARIABLE _lrelease_version
  OUTPUT_STRIP_TRAILING_WHITESPACE)
message(STATUS "caret i18n lrelease：${_lrelease_version}")

file(GLOB CARET_I18N_TS_FILES "${CARET_I18N_TS_DIR}/*.ts")
if(NOT CARET_I18N_TS_FILES)
  message(FATAL_ERROR "${CARET_I18N_TS_DIR} 下没有 .ts")
endif()

set(CARET_I18N_QM_FILES)
foreach(_ts IN LISTS CARET_I18N_TS_FILES)
  get_filename_component(_stem "${_ts}" NAME_WE)
  set(_qm "${CMAKE_CURRENT_BINARY_DIR}/${_stem}.qm")
  add_custom_command(
    OUTPUT "${_qm}"
    COMMAND "${CARET_LRELEASE_EXECUTABLE}" "${_ts}" -qm "${_qm}"
    DEPENDS "${_ts}"
    COMMENT "lrelease ${_stem}"
    VERBATIM)
  list(APPEND CARET_I18N_QM_FILES "${_qm}")
endforeach()

set(_i18n_qrc "${CMAKE_CURRENT_BINARY_DIR}/caret_i18n.qrc")
set(_i18n_qrc_entries)
foreach(_qm IN LISTS CARET_I18N_QM_FILES)
  get_filename_component(_qm_name "${_qm}" NAME)
  string(APPEND _i18n_qrc_entries "    <file>${_qm_name}</file>\n")
endforeach()
file(
  WRITE "${_i18n_qrc}"
  "<!DOCTYPE RCC>\n<RCC version=\"1.0\">\n  <qresource prefix=\"/i18n\">\n${_i18n_qrc_entries}  </qresource>\n</RCC>\n"
)

qt_add_resources(CARET_I18N_RESOURCE_SOURCES "${_i18n_qrc}")
add_library(caret_i18n OBJECT ${CARET_I18N_RESOURCE_SOURCES})
caret_native_defaults(caret_i18n)
add_library(caret::i18n ALIAS caret_i18n)

if(BUILD_TESTING)
  add_executable(caret_i18n_qm_test ../../tests/cpp/i18n/qm_load_test.cpp)
  caret_native_defaults(caret_i18n_qm_test)
  target_link_libraries(caret_i18n_qm_test PRIVATE caret_i18n Qt6::Core Qt6::Test)
  add_test(NAME I18n.CompiledQmLoads COMMAND caret_i18n_qm_test)
  set_tests_properties(I18n.CompiledQmLoads PROPERTIES LABELS "cpp;i18n" ENVIRONMENT
                                                       "QT_QPA_PLATFORM=offscreen")
endif()
