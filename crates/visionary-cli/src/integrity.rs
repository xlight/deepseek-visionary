//! Windows 完整性级别（mandatory integrity level）探测。
//!
//! # 为什么需要它
//!
//! DSH / Codex 沙箱会给**工作区目录**打上 `Mandatory Label\Low` 标签
//! （带 `(OI)(CI)` 继承）。工作区里**新构建出来的可执行文件**因此继承同一个
//! Low 标签 → 进程以 **Low 完整性**运行 → 它派生出来的 `chrome.exe` 也只能
//! 以 Low 完整性启动 → Chrome 初始化渲染进程沙箱时 CHECK 失败 → 进程直接
//! `__debugbreak()` 退出，退出码 `0x80000003`（STATUS_BREAKPOINT，stderr 为空）。
//!
//! 这个现象和「Chrome 装坏了 / 需要手动启动 / 需要关掉某个安全软件」都无关：
//! **同一个二进制文件，只要把它的完整性标签从 Low 改成 Medium，Chrome 立刻
//! 能正常拉起**。npm 全局安装目录不受沙箱标签影响，所以发布版一直正常。
//!
//! 这里只做**探测**（读当前进程令牌的有效完整性级别），用于给出准确诊断。
//! 真正的修复是把可执行文件的标签改回 Medium，见 `scripts/fix-integrity.ps1`。

/// 进程令牌的有效完整性级别。
///
/// 取值与 Windows 的完整性 RID 对应（`TOKEN_MANDATORY_LABEL` 里 SID 的
/// 最后一个子权限值）。
// 变体完整映射 Windows 的完整性 RID，但只有 Windows 分支的 current_level()
// 会真正构造它们；非 Windows 上该探针恒为 None，clippy -D warnings 会误报
// dead_code。仅在非 Windows 放宽，Windows 下仍严格检查。
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrityLevel {
    /// 0x0000 — Untrusted。
    Untrusted,
    /// 0x1000 — Low。**沙箱工作区里的构建产物默认落在这里**。
    Low,
    /// 0x2000 — Medium。普通交互式进程的默认值，也是 Chrome 正常工作的前提。
    Medium,
    /// 0x3000 — High。
    High,
    /// 0x4000 — System。
    System,
    /// 0x5000 — Protected。
    Protected,
}

impl IntegrityLevel {
    /// 人类可读的名字（用于诊断输出）。
    pub fn name(self) -> &'static str {
        match self {
            IntegrityLevel::Untrusted => "Untrusted",
            IntegrityLevel::Low => "Low",
            IntegrityLevel::Medium => "Medium",
            IntegrityLevel::High => "High",
            IntegrityLevel::System => "System",
            IntegrityLevel::Protected => "Protected",
        }
    }

    /// 是否低于 Medium —— Chrome 派生进程会因此启动失败。
    pub fn is_below_medium(self) -> bool {
        self < IntegrityLevel::Medium
    }
}

#[cfg(windows)]
mod imp {
    use super::IntegrityLevel;
    use std::ffi::c_void;

    type Handle = *mut c_void;

    const TOKEN_QUERY: u32 = 0x0008;
    // TokenIntegrityLevel
    const TOKEN_INTEGRITY_LEVEL: i32 = 25;

    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn OpenProcessToken(process: Handle, desired_access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            info_class: i32,
            info: *mut c_void,
            info_len: u32,
            return_len: *mut u32,
        ) -> i32;
        fn CloseHandle(object: Handle) -> i32;
    }

    /// 读当前进程令牌的有效完整性级别。
    pub(super) fn current_level() -> Option<IntegrityLevel> {
        let mut token: Handle = std::ptr::null_mut();
        // SAFETY: token 是有效的输出指针；OpenProcessToken 失败时返回 0。
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }

        let mut needed: u32 = 0;
        // SAFETY: 先用 null buffer 查询所需长度（标准两段式调用）。
        unsafe {
            GetTokenInformation(
                token,
                TOKEN_INTEGRITY_LEVEL,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        if needed == 0 {
            // SAFETY: token 是本函数成功打开的句柄。
            unsafe { CloseHandle(token) };
            return None;
        }

        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe {
            GetTokenInformation(
                token,
                TOKEN_INTEGRITY_LEVEL,
                buf.as_mut_ptr() as *mut c_void,
                needed,
                &mut needed,
            )
        };
        // SAFETY: 同上，句柄由本函数打开。
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return None;
        }

        // TOKEN_MANDATORY_LABEL = { SID_AND_ATTRIBUTES Label; }
        // SID_AND_ATTRIBUTES = { PSID Sid; DWORD Attributes; }
        // → 结构体偏移 0 处就是 Sid 指针。
        // SAFETY: buf 至少有一个 TOKEN_MANDATORY_LABEL 大小。
        let sid = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const *const u8) };
        if sid.is_null() {
            return None;
        }
        // SID 头部：Revision(1) + SubAuthorityCount(1) + IdentifierAuthority(6)
        // → 子权限数组从偏移 8 开始，每个 4 字节；完整性 RID 是最后一个。
        // SAFETY: sid 由系统返回，至少包含 8 字节头部。
        let count = unsafe { *sid.add(1) } as usize;
        if count == 0 {
            return None;
        }
        // SAFETY: SID 的子权限数组长度由 count 给出。
        let rid = unsafe { std::ptr::read_unaligned(sid.add(8 + (count - 1) * 4) as *const u32) };

        Some(match rid {
            0x0000 => IntegrityLevel::Untrusted,
            0x1000 => IntegrityLevel::Low,
            0x2000 => IntegrityLevel::Medium,
            0x3000 => IntegrityLevel::High,
            0x4000 => IntegrityLevel::System,
            0x5000 => IntegrityLevel::Protected,
            _ => return None,
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::IntegrityLevel;

    pub(super) fn current_level() -> Option<IntegrityLevel> {
        None
    }
}

/// 当前进程令牌的有效完整性级别。
///
/// 非 Windows 平台返回 `None`（该概念不存在）。
pub fn current_level() -> Option<IntegrityLevel> {
    imp::current_level()
}

/// 当前进程是否运行在低于 Medium 的完整性级别。
///
/// 为 `true` 时，派生出来的浏览器会以同样的低完整性启动并立刻崩溃。
pub fn running_below_medium() -> bool {
    current_level()
        .map(|l| l.is_below_medium())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ordering_puts_medium_above_low() {
        assert!(IntegrityLevel::Low < IntegrityLevel::Medium);
        assert!(IntegrityLevel::Low.is_below_medium());
        assert!(!IntegrityLevel::Medium.is_below_medium());
        assert!(!IntegrityLevel::High.is_below_medium());
    }

    #[test]
    fn level_names_are_stable() {
        assert_eq!(IntegrityLevel::Low.name(), "Low");
        assert_eq!(IntegrityLevel::Medium.name(), "Medium");
    }

    #[test]
    fn current_level_probe_does_not_panic() {
        // 只断言「能问出答案或明确问不出」，不假设具体级别（CI 里可能是 Low）。
        let _ = current_level();
        let _ = running_below_medium();
    }
}
