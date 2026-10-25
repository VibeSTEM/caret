line_width = 100
command_case = 'unchanged'
keyword_case = 'unchanged'
enable_sort = False
autosort = False
max_pargs_hwrap = 6
dangle_parens = False
enable_markup = False

# 现有 CMake 使用前导下划线表示局部/私有变量，并有意保留长 URL 与
# 生成命令行。保留这些项目约定，同时让 cmake-lint 继续检查可修复的
# 语法和规则问题。
global_var_pattern = r'[A-Za-z][A-Za-z0-9_]*'
internal_var_pattern = r'_?[A-Za-z][A-Za-z0-9_]*'
local_var_pattern = r'_?[a-z][a-z0-9_]*'
private_var_pattern = r'_?[a-z][a-z0-9_]*'
argument_var_pattern = r'_?[a-z][a-z0-9_]*'
# C0103 在此关闭，因为 sdk-provision.cmake 会根据 manifest 键动态构造
# 私有变量名（例如 ``_${_k}``）。
disabled_codes = ['C0103', 'C0111', 'C0301', 'R0912', 'R0915']
