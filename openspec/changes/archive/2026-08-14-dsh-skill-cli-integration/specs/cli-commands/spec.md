# cli-commands Delta

## MODIFIED Requirements

### Requirement: agent 调用契约

项目 SHALL 提供 agent 调用契约文档（`skills/visionary-cli/SKILL.md`），描述 `vision` 子命令的用法、`--json` 输出形状与退出码约定。契约 SHALL 规定 agent 调用 `vision` 时使用 `--json` 原子输出（而非解析流式文本），并说明非 TTY 环境默认原子输出的兑底行为。

`skill` 子命令 SHALL 将内嵌于二进制的 SKILL.md（`include_str!`）写入 `~/.agents/skills/visionary-cli/SKILL.md`（自动创建目录），使通过安装脚本装二进制的用户无需仓库即可获得 skill。已存在时 SHALL 覆盖并提示。写入失败时 SHALL 输出错误并退出非零。

skill 的安装逻辑 SHALL 抽为可复用函数（输入目标技能根目录，输出写入路径），供 `init dsh` 复用：`init dsh` SHALL 经同一函数将内嵌 SKILL.md 安装到 `$DSH_HOME/skills/visionary-cli/SKILL.md`（DSH user 技能根）与 `~/.agents/skills/visionary-cli/SKILL.md`。`skill install` 自身行为不变（仍只写 `~/.agents/skills/visionary-cli/SKILL.md`）。

#### Scenario: agent 通过 skill 调用
- **WHEN** agent 读取 `skills/visionary-cli/SKILL.md` 后调用 `visionary-server vision img.png --json`
- **THEN** agent 获得原子 JSON 输出（含 text / session_id / parent_message_id），可可靠解析

#### Scenario: skill install 安装
- **WHEN** 用户执行 `visionary-server skill install`
- **THEN** 程序将内嵌 SKILL.md 写入 `~/.agents/skills/visionary-cli/SKILL.md`（自动创建目录），输出安装路径，退出码为 0

#### Scenario: skill install 覆盖既有文件
- **WHEN** 用户执行 `visionary-server skill install` 且目标文件已存在
- **THEN** 程序覆盖写入并提示已更新，退出码为 0

#### Scenario: init dsh 复用 skill 安装逻辑
- **WHEN** 用户执行 `visionary-server init dsh`
- **THEN** 程序经与 `skill install` 相同的安装函数将内嵌 SKILL.md 写入 `$DSH_HOME/skills/visionary-cli/SKILL.md` 与 `~/.agents/skills/visionary-cli/SKILL.md`，两处文件内容与内嵌一致
