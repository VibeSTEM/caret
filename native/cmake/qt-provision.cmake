# Qt 6.12.0 预编译供给。官方在线仓库的归档名与 MirrorBrain SHA-256。
# 不源码构建。解包使用 cmake -E tar（libarchive 支持 7z）。
# 官方 download.qt.io 失败时回退清华 TUNA；CARET_QT_MIRROR 可插到最前。
# SHA256 与来源无关，始终强制。
# 供给目录是仓库根 .toolchain/qt（或 CARET_TOOLCHAIN_ROOT/qt），各 profile 共用。
# caret-build::ensure_qt 与 CMake include 共用本脚本。
# 模块：qtbase、qtdeclarative、qttools（lrelease）、qtsvg、qtscxml。
# StateMachine 由设置窗口交互控制器使用。

if(NOT DEFINED QT_PROVISION_DIR)
  get_filename_component(_caret_repo_root "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
  set(QT_PROVISION_DIR "${_caret_repo_root}/.toolchain/qt")
endif()
# Cargo build 与独立格式检查可能同时准备 Qt；进程退出自动释放锁。
file(MAKE_DIRECTORY "${QT_PROVISION_DIR}")
file(
  LOCK "${QT_PROVISION_DIR}/provision.lock"
  GUARD FILE
  TIMEOUT 900)
set(QT_STAGING "${QT_PROVISION_DIR}/staging")
# 每个归档解包完成后的指纹目录；staging 被清空时全部重解。
set(QT_EXTRACTED "${QT_PROVISION_DIR}/extracted")

if(APPLE)
  set(_qt_repo_path "mac_x64/desktop/qt6_6120/qt6_6120/qt.qt6.6120.clang_64")
  set(_qt_addon_repo_path "mac_x64/desktop/qt6_6120/qt6_6120/qt.qt6.6120.addons.qtscxml.clang_64")
  set(_qt_addon_archives
      "6.12.0-0-202609280346qtscxml-MacOS-MacOS_26-Clang-MacOS-MacOS_26-X86_64-ARM64.7z|7a011bb00213e17e12e7e3aab3aee65a447bd44290f5de3d29f6cb6c8961c8ba"
  )
  set(_qt_archives
      "6.12.0-0-202609280346qtbase-MacOS-MacOS_26-Clang-MacOS-MacOS_26-X86_64-ARM64.7z|1e0df66ed17ae97997971ff481983c5c0b7497a52ef52f699725701401fd80ee"
      "6.12.0-0-202609280346qtdeclarative-MacOS-MacOS_26-Clang-MacOS-MacOS_26-X86_64-ARM64.7z|6b070cade4201db28ed34f65331085c3916a718bc6a30ea24c2f57e67debacea"
      "6.12.0-0-202609280346qttools-MacOS-MacOS_26-Clang-MacOS-MacOS_26-X86_64-ARM64.7z|c503c114fbb96a2c2dad07fae4d195cb800ab498c9deb440c879e657ec14e8f8"
      "6.12.0-0-202609280346qtsvg-MacOS-MacOS_26-Clang-MacOS-MacOS_26-X86_64-ARM64.7z|428c6d02a7bdaf832463aa97a19c1c0920a8e840c751499d05fa9e43dc69f172"
  )
elseif(WIN32)
  set(_qt_repo_path
      "windows_x86/desktop/qt6_6120/qt6_6120_msvc2022_64/qt.qt6.6120.win64_msvc2022_64")
  set(_qt_addon_repo_path
      "windows_x86/desktop/qt6_6120/qt6_6120_msvc2022_64/qt.qt6.6120.addons.qtscxml.win64_msvc2022_64"
  )
  set(_qt_addon_archives
      "6.12.0-0-202609280346qtscxml-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|686d38b961a31de55dfc10d3177827ba0691fce1d642a058d7bb5614ad13671a"
  )
  set(_qt_archives
      "6.12.0-0-202609280346qtbase-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|219d1b86def3be418e31d704572bc4acc91b2ec373c65595c9fb12b42bf098dc"
      "6.12.0-0-202609280346qtdeclarative-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|258146e9972fadbc1162cda31c7d7ed7e627f5022b2a848f2edb84d9c2ce27db"
      "6.12.0-0-202609280346qttools-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|73328e361429378df518f391ebe4d82c51e833f4d5a951692768ab5a5a1d7da5"
      "6.12.0-0-202609280346qtsvg-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|dfcad02224b71724e3314d0bc3ffc0b862dfa6e02bc4aadd39f1738f6c364c79"
  )
elseif(UNIX)
  set(_qt_repo_path "linux_x64/desktop/qt6_6120/qt6_6120/qt.qt6.6120.linux_gcc_64")
  set(_qt_addon_repo_path
      "linux_x64/desktop/qt6_6120/qt6_6120/qt.qt6.6120.addons.qtscxml.linux_gcc_64")
  set(_qt_addon_archives
      "6.12.0-0-202609280346qtscxml-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|64f368903ead49900f76346a0de50d8348b354bf10cf1b38aca61a7f48d3a096"
  )
  set(_qt_archives
      "6.12.0-0-202609280346qtbase-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|943023b929d0c08eafe7341700b5ed7601284410b7afd6093f950be291d1d7cd"
      "6.12.0-0-202609280346qtdeclarative-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|0c719e17ea9fbd37aa615595149dca2578985656433f27bf2846cffe4b687d3e"
      "6.12.0-0-202609280346qttools-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|5d6f90961df3a1e5a57fac69bbbe6983860bad11c419a49736b84914588b4334"
      "6.12.0-0-202609280346qtsvg-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|22a7404505a2883c54556183f19adfff1cdbea30664e34bfd4171180802142e9"
  )
  # Qt Linux 工具使用与该发行版配套的 ICU 73.2。此归档由 Qt 官方仓库提供，
  # 文件直接放入 staging/lib，供 rcc、qtpaths、qmlimportscanner 等工具通过
  # $ORIGIN/../lib 解析；不使用系统 ICU，也不在本地编译 ICU。
  set(_qt_runtime_archives
      "6.12.0-0-202609280346icu-linux-Rhel8.6-x86_64.7z|1fdbcd67b1760f3a3f031c64637a9f19992aed2f35aa615e12a468f8c655d4da"
  )
else()
  message(FATAL_ERROR "Qt 预编译供给暂不支持平台：${CMAKE_SYSTEM_NAME}（记录到 dependency-acquisition.md 再扩展）")
endif()

# 下载来源顺序：官方源先行；CARET_QT_MIRROR 设置时插到最前——接受两种
# 取值：镜像站点根（如 https://mirrors.tuna.tsinghua.edu.cn/qt，自动补
# /online/qtsdkrepository）或已含 qtsdkrepository 的完整仓库根。
set(_qt_source_roots "https://download.qt.io/online/qtsdkrepository"
                     "https://mirrors.tuna.tsinghua.edu.cn/qt/online/qtsdkrepository")
if(DEFINED ENV{CARET_QT_MIRROR} AND NOT "$ENV{CARET_QT_MIRROR}" STREQUAL "")
  set(_qt_mirror_root "$ENV{CARET_QT_MIRROR}")
  string(REGEX REPLACE "/$" "" _qt_mirror_root "${_qt_mirror_root}")
  if(NOT _qt_mirror_root MATCHES "qtsdkrepository")
    string(APPEND _qt_mirror_root "/online/qtsdkrepository")
  endif()
  list(PREPEND _qt_source_roots "${_qt_mirror_root}")
endif()
# 一轮 configure 内回退成功过的来源，后续归档直接优先使用。
set(_qt_preferred_root "")

# 校验/下载/解包单个 "归档名|SHA256" 条目：残件哈希不符即删并重下；按
# 来源顺序下载（STATUS 捕获失败回退下一来源，EXPECTED_HASH 全程强校验）；
# 解包到 workdir 后即删归档，缓存只保留解包树与调用方维护的指纹。成功
# 来源经 PARENT_SCOPE 回传为后续归档的优先来源。
function(caret_qt_fetch_extract entry repo_path workdir)
  string(REPLACE "|" ";" _kv "${entry}")
  list(GET _kv 0 _archive_name)
  list(GET _kv 1 _archive_sha)
  set(_archive_path "${QT_PROVISION_DIR}/archives/${_archive_name}")
  if(EXISTS "${_archive_path}")
    file(SHA256 "${_archive_path}" _archive_actual)
    if(NOT _archive_actual STREQUAL _archive_sha)
      file(REMOVE "${_archive_path}")
      message(STATUS "Qt 归档校验不符，已删除并重新下载：${_archive_name}")
    endif()
  endif()
  if(NOT EXISTS "${_archive_path}")
    set(_order "${_qt_source_roots}")
    if(_qt_preferred_root)
      list(REMOVE_ITEM _order "${_qt_preferred_root}")
      list(PREPEND _order "${_qt_preferred_root}")
    endif()
    set(_last_status "未尝试任何来源")
    foreach(_root IN LISTS _order)
      message(STATUS "下载 Qt 归档：${_archive_name}（${_root}）")
      file(
        DOWNLOAD "${_root}/${repo_path}/${_archive_name}" "${_archive_path}"
        STATUS _download_status
        INACTIVITY_TIMEOUT 120
        TIMEOUT 900
        EXPECTED_HASH SHA256=${_archive_sha})
      list(GET _download_status 0 _download_code)
      if(_download_code EQUAL 0)
        set(_qt_preferred_root
            "${_root}"
            PARENT_SCOPE)
        break()
      endif()
      file(REMOVE "${_archive_path}")
      set(_last_status "${_download_status}")
    endforeach()
    if(NOT EXISTS "${_archive_path}")
      message(FATAL_ERROR "Qt 归档下载失败：${_archive_name}；末次状态 ${_last_status}；"
                          "可设置 CARET_QT_MIRROR 指向其他镜像后重试")
    endif()
  endif()
  message(STATUS "解包 Qt 归档：${_archive_name}")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E tar xf "${_archive_path}"
    WORKING_DIRECTORY "${workdir}"
    RESULT_VARIABLE _extract_result)
  if(NOT _extract_result EQUAL 0)
    message(FATAL_ERROR "Qt 归档解包失败（${_extract_result}）：${_archive_name}")
  endif()
  # 解包成功即删归档，避免与解包树在缓存中长期双份；staging 损坏时按
  # 调用方指纹或探测文件重新走本流程。
  file(REMOVE "${_archive_path}")
endfunction()

# 将全部平台 kit 组件作为一个缓存单元。升级或回退时清除共享 staging 与
# 组件指纹，避免只覆盖新版文件、遗留旧版文件，或因旧指纹跳过回退解包。
string(
  SHA256
    _qt_kit_sha
    "${_qt_repo_path}\n${_qt_addon_repo_path}\n${_qt_archives}\n${_qt_addon_archives}\n${_qt_runtime_archives}"
)
set(_qt_kit_marker "${QT_PROVISION_DIR}/kit.sha256")
set(_qt_reset_staging TRUE)
if(EXISTS "${_qt_kit_marker}")
  file(STRINGS "${_qt_kit_marker}" _recorded_kit_sha)
  if(_recorded_kit_sha STREQUAL "${_qt_kit_sha}" AND EXISTS "${QT_STAGING}/bin")
    set(_qt_reset_staging FALSE)
  endif()
endif()
if(_qt_reset_staging)
  file(REMOVE_RECURSE "${QT_STAGING}" "${QT_EXTRACTED}")
  file(REMOVE "${_qt_kit_marker}")
endif()
file(MAKE_DIRECTORY "${QT_PROVISION_DIR}/archives" "${QT_EXTRACTED}" "${QT_STAGING}")
foreach(entry IN LISTS _qt_archives)
  string(REPLACE "|" ";" _kv "${entry}")
  list(GET _kv 0 _archive_name)
  list(GET _kv 1 _archive_sha)
  set(_extract_marker "${QT_EXTRACTED}/${_archive_name}.sha256")
  # 已按同一哈希解包且 staging 仍完整则跳过（归档发布即删，复用不依赖
  # 归档）；指纹不符（升级归档或清空 staging）才重新下载/解包，老缓存只补
  # 新模块。
  if(EXISTS "${_extract_marker}" AND EXISTS "${QT_STAGING}/bin")
    file(STRINGS "${_extract_marker}" _recorded_sha)
    if(_recorded_sha STREQUAL "${_archive_sha}")
      continue()
    endif()
  endif()
  caret_qt_fetch_extract("${entry}" "${_qt_repo_path}" "${QT_STAGING}")
  file(WRITE "${_extract_marker}" "${_archive_sha}\n")
endforeach()

# Qt StateMachine 是在线安装器的 qtscxml 附加包提供的独立模块。它与主 kit
# 归档分目录发布，因此单独登记来源并复用相同 SHA256、解包与缓存指纹策略。
foreach(entry IN LISTS _qt_addon_archives)
  string(REPLACE "|" ";" _kv "${entry}")
  list(GET _kv 0 _archive_name)
  list(GET _kv 1 _archive_sha)
  set(_extract_marker "${QT_EXTRACTED}/${_archive_name}.sha256")
  if(EXISTS "${_extract_marker}" AND EXISTS "${QT_STAGING}/bin")
    file(STRINGS "${_extract_marker}" _recorded_sha)
    if(_recorded_sha STREQUAL "${_archive_sha}")
      continue()
    endif()
  endif()
  caret_qt_fetch_extract("${entry}" "${_qt_addon_repo_path}" "${QT_STAGING}")
  file(WRITE "${_extract_marker}" "${_archive_sha}\n")
endforeach()

if(DEFINED _qt_runtime_archives)
  foreach(entry IN LISTS _qt_runtime_archives)
    string(REPLACE "|" ";" _kv "${entry}")
    list(GET _kv 0 _archive_name)
    list(GET _kv 1 _archive_sha)
    set(_extract_marker "${QT_EXTRACTED}/${_archive_name}.sha256")
    if(EXISTS "${_extract_marker}" AND EXISTS "${QT_STAGING}/lib/libicui18n.so.73")
      file(STRINGS "${_extract_marker}" _recorded_sha)
      if(_recorded_sha STREQUAL "${_archive_sha}")
        continue()
      endif()
    endif()
    caret_qt_fetch_extract("${entry}" "${_qt_repo_path}" "${QT_STAGING}/lib")
    file(WRITE "${_extract_marker}" "${_archive_sha}\n")
  endforeach()
endif()

# 仅完整供给成功后提交 kit 指纹；中断的本地 staging 下次会重新创建。
file(WRITE "${_qt_kit_marker}" "${_qt_kit_sha}\n")

unset(_qt_repo_path)
unset(_qt_addon_repo_path)
unset(_qt_addon_archives)
unset(_qt_archives)
unset(_qt_source_roots)
unset(_qt_mirror_root)
unset(_qt_preferred_root)
unset(_archive_path)
unset(_archive_name)
unset(_archive_sha)
unset(_qt_runtime_archives)
unset(_extract_marker)
unset(_recorded_sha)
unset(_qt_kit_sha)
unset(_qt_kit_marker)
unset(_recorded_kit_sha)
unset(_qt_reset_staging)
