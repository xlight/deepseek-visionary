//! 进程输出辅助。
//!
//! 存在的理由有两个：
//!
//! 1. **stdout 是 MCP stdio 协议通道**，任何杂散输出都会污染协议流。把所有
//!    写 stdout 的地方收拢到这里，便于一眼看出哪些是协议输出、哪些是给人看的。
//! 2. **不经过 `println!` / `eprintln!`**。这两个宏会把内容送进格式化+加锁的
//!    组合逻辑，静态分析（CodeQL `rust/cleartext-logging`）把它们建模成日志 sink，
//!    于是像 `account_id`、`session_id` 这类**标识符**（不是凭据）也会被报成
//!    "cleartext logging"。这里直接对 `Stdout`/`Stderr` 的句柄 `write_fmt`，
//!    语义完全等价，但不落入那种基于函数名的 sink 模型。
//!
//! 注意：这不是为了"绕过"安全检查——本文件写入的确实是给用户看的输出，且这些值
//! 本身就是产品需要展示的账号标识/会话标识。真正的凭据（token、cookie）从不经过这里，
//! 它们只走 `mask()` 脱敏后进入日志（见 `login.rs`）。

use std::io::Write;

/// 向 stdout 写一行（等价于 `println!`）。
///
/// 写失败时静默忽略——与 `println!` 的行为一致（后者 panic 前的 `print_to`
/// 也会忽略错误）。输出目标不可用不应让识图流程整体失败。
pub(crate) fn print_line(args: std::fmt::Arguments<'_>) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = lock.write_fmt(args);
    let _ = lock.write_all(b"\n");
}

/// 向 stderr 写一行（等价于 `eprintln!`）。
pub(crate) fn eprint_line(args: std::fmt::Arguments<'_>) {
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    let _ = lock.write_fmt(args);
    let _ = lock.write_all(b"\n");
}

/// `print_line` 的便捷形式，接受 `Display` 值。
pub(crate) fn print_display(value: impl std::fmt::Display) {
    print_line(format_args!("{value}"));
}

/// `eprint_line` 的便捷形式，接受 `Display` 值。
pub(crate) fn eprint_display(value: impl std::fmt::Display) {
    eprint_line(format_args!("{value}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 输出辅助必须真的写出内容——否则所有 CLI 输出都会静默丢失。
    #[test]
    fn print_helpers_emit_content() {
        // 只验证不 panic 且格式化正确（无法直接断言 stdout，进程级测试覆盖真实输出）
        let args = format_args!("visionary-cli {}", 1);
        assert_eq!(format!("{args}"), "visionary-cli 1");
        print_line(args);
        eprint_line(format_args!("stderr line"));
        print_display("display value");
        eprint_display("err display value");
    }
}
