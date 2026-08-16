# visionary-server (CLI)

## ADDED Requirements

### Requirement: Windows 下用户目录解析不依赖 HOME 环境变量

Windows PowerShell / cmd 环境没有 `HOME` 环境变量（用户目录在 `USERPROFILE`）。CLI 的所有用户目录解析 SHALL 依次回退 `HOME` → `USERPROFILE`，两者皆缺失时才报错。

#### Scenario: Windows 无 HOME 时 login 可正常定位配置目录

- **WHEN** 环境只有 `USERPROFILE`（无 `HOME`），运行 `visionary-server status`
- **THEN** 数据目录解析为 `<USERPROFILE>\.deepseek-visionary`，命令正常执行（不再报 "HOME not set"）

#### Scenario: 两者皆缺失时报明确错误

- **WHEN** 环境既无 `HOME` 也无 `USERPROFILE`
- **THEN** 命令报错 `HOME/USERPROFILE not set` 且退出非零

### Requirement: skill install 在 Windows 下安装到用户目录

`visionary-server skill install` SHALL 使用与数据目录相同的跨平台 home 解析，而非回退到当前工作目录。

#### Scenario: Windows 无 HOME 时 skill 安装到 USERPROFILE

- **WHEN** 环境只有 `USERPROFILE`（无 `HOME`），运行 `visionary-server skill install`
- **THEN** skill 写入 `<USERPROFILE>\.agents\skills\visionary-cli\SKILL.md`，退出 0
