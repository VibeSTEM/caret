# 006 — CI 与打包管线

- 状态：ready
- 依赖：005
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

005 之后仓库里已经有 Rust、QML 和 native 入口。本任务把来源的质量、夜间、打包和 GoogleTest SDK 工作流移植过来，并改成 caret 的名字和 `.toolchain/` 缓存。移植完成后，基础设施阶段结束。

## 必读

- [持续集成](../standards/ci.md)、[依赖获取](../standards/dependency-acquisition.md)、[测试规范](../standards/testing.md)
- [Cargo](../standards/cargo.md)

## 范围与非目标

移植 `ci.yml`、`nightly.yml`、`package.yml`、`sdk-googletest.yml` 和配套 composite action。环境变量 `PANTA_*` 改为 `CARET_*`。工具缓存指向 `.toolchain/`。

不移植几何、网格和渲染 SDK 的工作流。不把夜间覆盖率改成编辑器性能门禁。函数和行覆盖率门槛仍是 85%，只在夜间阻断。

## 前置条件与待决策

005 已 done，空白窗口的构建是 CI 要覆盖的 native 目标。

## 实施步骤

1. 移植 workflow 和 action，按规范改名、改缓存键。记录来源 commit。
2. 核对纯文档变更不创建运行，未知路径扩大到完整范围。
3. 本地跑通 CI 会调用的 fmt、clippy 和已有测试。workflow 本身在推送或 `gh` 手动运行后把结论写回本文件。
4. 确认没有来源前缀残留在 `.github/`。
5. 独立评审后复验。

## 预计改动

新建 `.github/workflows/` 和 `.github/actions/`。不改编辑器行为。

## 验收标准

- [ ] 四条要移植的 workflow 都在，排除的 SDK workflow 不在。
- [ ] `.github/` 里没有 `panta` / `PANTA_` 前缀。
- [ ] 缓存键不包含 `target/` 里的工具目录。
- [ ] 本地 fmt、clippy 和已有测试通过。远程运行若尚未推送，验证表写明未执行，不记为通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 检索 `.github/` 的来源前缀 | 无匹配 | 未执行 |
| — | 本地 fmt、clippy、已有测试 | 通过 | 未执行 |
| — | 推送后的 workflow | 待运行 | 未执行 |

## 风险与回退

来源 workflow 假设的目录这阶段已经存在。若某一步仍引用排除掉的 SDK，删掉该步，不把 SDK 加回来。回退是移除 `.github/workflows` 中对应文件。

## 决策与工作记录

- 2026-10-25：创建任务。CI 放在 launcher 之后，这样第一次远程运行面对的是能编译的树。

## 完成摘要

未完成。
