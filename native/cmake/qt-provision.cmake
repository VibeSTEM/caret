# Qt 6.11.2 预编译供给。来源仓库 panta ee84d95 的官方归档与 SHA256。
# 不源码构建。解包使用 cmake -E tar（libarchive 支持 7z）。
# 官方 download.qt.io 失败时回退清华 TUNA；CARET_QT_MIRROR 可插到最前。
# SHA256 与来源无关，始终强制。
# 供给目录是仓库根 .toolchain/qt（或 CARET_TOOLCHAIN_ROOT/qt），各 profile 共用。
# caret-build::ensure_qt 与 CMake include 共用本脚本。
# 模块：qtbase、qtdeclarative、qttools（lrelease）、qtsvg、qtscxml。
# StateMachine 只在以后的界面桥里查找，本骨架不链接它。

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
  set(_qt_repo_path "mac_x64/desktop/qt6_6112/qt6_6112/qt.qt6.6112.clang_64")
  set(_qt_addon_repo_path "mac_x64/desktop/qt6_6112/qt6_6112/qt.qt6.6112.addons.qtscxml.clang_64")
  set(_qt_addon_archives
      "6.11.2-0-202608131016qtscxml-MacOS-MacOS_15-Clang-MacOS-MacOS_15-X86_64-ARM64.7z|9cabcf73b1ef81cb396a205380950d947535e20b7b6e21204e59a52199a98bfe"
  )
  set(_qt_archives
      "6.11.2-0-202608131016qtbase-MacOS-MacOS_15-Clang-MacOS-MacOS_15-X86_64-ARM64.7z|9592f84f7e26d532c5c56824d1da7c9214a766cb0a17beb5af71022bcfbcd271"
      "6.11.2-0-202608131016qtdeclarative-MacOS-MacOS_15-Clang-MacOS-MacOS_15-X86_64-ARM64.7z|ceb8e3f3830531a52de5007ef3f50ffc5525021feb132ee322f59583eb2903cf"
      "6.11.2-0-202608131016qttools-MacOS-MacOS_15-Clang-MacOS-MacOS_15-X86_64-ARM64.7z|415b5008059d0066ac0d4806de98f6e8dacf7b5c80ca0305875160d2107d3719"
      "6.11.2-0-202608131016qtsvg-MacOS-MacOS_15-Clang-MacOS-MacOS_15-X86_64-ARM64.7z|c52f6cec6ce4b52ca017e0046345a85d33b2b73e0f6a93474bb7a60f8e829317"
  )
elseif(WIN32)
  set(_qt_repo_path
      "windows_x86/desktop/qt6_6112/qt6_6112_msvc2022_64/qt.qt6.6112.win64_msvc2022_64")
  set(_qt_addon_repo_path
      "windows_x86/desktop/qt6_6112/qt6_6112_msvc2022_64/qt.qt6.6112.addons.qtscxml.win64_msvc2022_64"
  )
  set(_qt_addon_archives
      "6.11.2-0-202608131017qtscxml-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|47539713e42a72f29ef0da0be2ad82428eefb7158bfe766b2e1dc7e63640e058"
  )
  set(_qt_archives
      "6.11.2-0-202608131017qtbase-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|fd984b7264361b4dd3fd2a417702ca1258e4086268f2ee6a69b9a393d9c3f6bb"
      "6.11.2-0-202608131017qtdeclarative-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|5591ca564c1a9299a45a15b6b1d324c86e0aaed7cf6c47b7e6fe464aecae2587"
      "6.11.2-0-202608131017qttools-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|5f2b387a1f8055102b1388ff4ea743124ed6389d442f75c659728bb5c2a56946"
      "6.11.2-0-202608131017qtsvg-Windows-Windows_11_24H2-MSVC2022-Windows-Windows_11_24H2-X86_64.7z|417f44499c835b2303f3ff78043179bb23442f33d5d2816fbf7e8dbf275b1c89"
  )
elseif(UNIX)
  set(_qt_repo_path "linux_x64/desktop/qt6_6112/qt6_6112/qt.qt6.6112.linux_gcc_64")
  set(_qt_addon_repo_path
      "linux_x64/desktop/qt6_6112/qt6_6112/qt.qt6.6112.addons.qtscxml.linux_gcc_64")
  set(_qt_addon_archives
      "6.11.2-0-202608131018qtscxml-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|8579d82fba91dfe1b24e3203f740ac5647d5515c51182a4158b0e748dc6f00f7"
  )
  set(_qt_archives
      "6.11.2-0-202608131018qtbase-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|0f86f13b161141e77b1d056b54e2b6fc40fb16f243e71123346f9fb35d418027"
      "6.11.2-0-202608131018qtdeclarative-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|5f0ce87c077f749723dbb6e923142adb860c146ecaf93c68197feda5307f22dd"
      "6.11.2-0-202608131018qttools-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|42d5f3dbfc25647d9d95ef8b64401dc7e3ef7c83a39a29b548dfa0985f71c0ca"
      "6.11.2-0-202608131018qtsvg-Linux-RHEL_9_6-GCC-Linux-RHEL_9_6-X86_64.7z|939fe0e5d49d11d6d3eceea0184219703c25a1f8dc75f45e99071ca244824a0d"
  )
  # Qt Linux 工具使用与该发行版配套的 ICU 73。此归档由 Qt 官方仓库提供，
  # 文件直接放入 staging/lib，供 rcc、qtpaths、qmlimportscanner 等工具通过
  # $ORIGIN/../lib 解析；不使用系统 ICU，也不在本地编译 ICU。
  set(_qt_runtime_archives
      "6.11.2-0-202608131018icu-linux-Rhel8.6-x86_64.7z|111bdae30a66fff6ef65620e95766170fa5a6f425c360ea33b79cb2ec7e2fd86"
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

if(DEFINED _qt_runtime_archives AND NOT EXISTS "${QT_STAGING}/lib/libicui18n.so.73")
  foreach(entry IN LISTS _qt_runtime_archives)
    caret_qt_fetch_extract("${entry}" "${_qt_repo_path}" "${QT_STAGING}/lib")
  endforeach()
endif()

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
