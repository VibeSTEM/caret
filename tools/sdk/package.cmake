# 来源 panta ee84d95；只打包 GoogleTest 安装树，不进入普通构建。
if(NOT DEFINED PKG_ROOT OR NOT DEFINED PKG_DIST)
  message(FATAL_ERROR "package 需要 -DPKG_ROOT 与 -DPKG_DIST")
endif()
foreach(_key PKG_NAME PKG_VERSION PKG_TRIPLE)
  if(NOT DEFINED ${_key})
    message(FATAL_ERROR "package 需要 -D${_key}")
  endif()
endforeach()

file(MAKE_DIRECTORY "${PKG_DIST}")
set(_archive "${PKG_DIST}/${PKG_NAME}-${PKG_VERSION}-${PKG_TRIPLE}.tar.gz")
execute_process(
  # "." 打包含 ./include 等条目；解包后即平铺安装树。
  COMMAND "${CMAKE_COMMAND}" -E tar czf "${_archive}" .
  WORKING_DIRECTORY "${PKG_ROOT}"
  RESULT_VARIABLE _result)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "打包失败（退出码 ${_result}）：${_archive}")
endif()
file(SHA256 "${_archive}" _sha)
file(WRITE "${PKG_DIST}/${PKG_NAME}-${PKG_VERSION}-${PKG_TRIPLE}.sha256" "${_sha}\n")
message(STATUS "package：${_archive}（SHA256 ${_sha}）")
