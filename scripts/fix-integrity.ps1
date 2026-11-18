# 产出一份「Medium 完整性」的 visionary-cli 可执行文件。
#
# 背景：DSH / Codex 沙箱给工作区目录打上 Mandatory Label\Low（带 (OI)(CI) 继承），
# 工作区里构建出来的 visionary-cli.exe 继承 Low 标签。Low 完整性进程派生出的
# chrome.exe 同样以 Low 完整性启动，Chrome 初始化沙箱时 CHECK 失败，直接以
# 0x80000003（STATUS_BREAKPOINT）退出 —— 表现为「Chrome 打不开」。
#
# 为什么不直接改 target\release\visionary-cli.exe 的标签：
#   cargo 产出的顶层 exe 与 deps\visionary_server.exe 是**硬链接**（同一 MFT 条目），
#   标签共享。原地改成 Medium 会把 deps 副本也变成 Medium，而构建工具本身跑在
#   Low 完整性下 —— 链接器就再也打不开那个文件了（LNK1104 无法打开 ...\deps\visionary_server.exe）。
#   所以这里改成**复制**一份出来再改标签：产物独立、不与构建目录共享 MFT 条目。
#
# 用法（不需要管理员权限，只需要对源文件有读权限、对目标目录有写权限）：
#   powershell -ExecutionPolicy Bypass -File scripts\fix-integrity.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\fix-integrity.ps1 -Path <源 exe> -Out <目标 exe>

[CmdletBinding()]
param(
    # 源可执行文件；默认取本仓库的 release 构建产物。
    [string]$Path,
    # 输出路径；默认与源同目录，文件名加 .medium 后缀。
    [string]$Out
)

$ErrorActionPreference = 'Stop'

if (-not $Path) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $Path = Join-Path $scriptDir '..\target\release\visionary-cli.exe'
}
if (-not $Out) {
    $Out = [IO.Path]::Combine(
        [IO.Path]::GetDirectoryName($Path),
        [IO.Path]::GetFileNameWithoutExtension($Path) + '.medium.exe')
}

$src = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
if (-not $src) {
    Write-Error ('找不到源可执行文件：' + $Path + '（先跑 cargo build --release -p visionary-cli）')
    exit 1
}
$src = $src.Path

Write-Host ('源文件  : ' + $src)
Write-Host ('输出文件: ' + $Out)

# 复制成独立文件（不与源共享 MFT 条目），这样改标签不会影响构建目录里的产物。
Copy-Item -LiteralPath $src -Destination $Out -Force

$before = (icacls $Out | Select-String 'Mandatory Label' | ForEach-Object { $_.ToString().Trim() })
Write-Host ('复制后标签: ' + $(if ($before) { $before -join ' ' } else { '(无显式标签，继承目录)' }))

& icacls $Out /setintegritylevel 'M' /q
if ($LASTEXITCODE -ne 0) {
    Write-Error ('icacls 失败（exit ' + $LASTEXITCODE + '）。目标目录可能不可写，请换个输出路径。')
    exit $LASTEXITCODE
}

$after = (icacls $Out | Select-String 'Mandatory Label' | ForEach-Object { $_.ToString().Trim() })
Write-Host ('修复后标签: ' + ($after -join ' '))
Write-Host ('完成。运行：' + $Out + ' login')
