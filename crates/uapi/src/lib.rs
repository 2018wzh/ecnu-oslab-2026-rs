//! 用户态与内核态共享的 ABI: 系统调用号、错误码、返回值编码。
//! 系统调用号是跨特权级的契约, 用被两边共同依赖的 crate 保证一致。
//! `#![no_std]`: 用户程序与内核都是裸机, 都不能用 `std`。
//! 不放内核内部结构 —— 只放用户态可见的表面类型。
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

// 系统调用号。
// `#[repr(usize)]` 枚举而非一堆 const: match 可被检查穷尽性,
// 新增调用号忘记在分发器里处理会直接编译失败。
// 用 `usize` 而非 `u32`: a7 寄存器是 XLEN 宽, 并且保持 match 穷尽检查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Syscall {
    /// (lab-4..8) 内核打印固定字符串。不用 fd 表。
    HelloWorld = 0,
    /// 结束当前进程。`a0` = 退出码。
    Exit = 1,
    /// 复制当前进程。返回: 父进程得到子进程 pid, 子进程得到 0。
    Fork = 2,
    /// 读。`a0` = fd, `a1` = buf, `a2` = len。返回读到的字节数。
    Read = 3,
    /// 写。`a0` = fd, `a1` = buf, `a2` = len。返回写出的字节数。
    Write = 4,
    /// 用新映像替换当前进程。`a0` = 文件名指针。
    Exec = 5,
    /// 等待子进程退出。`a0` = pid, `a1` = 存放退出码的地址。
    Wait = 6,
    /// 内存映射。`a0` = 长度, 返回映射到的虚拟地址。
    Mmap = 9,
    /// 取当前进程号 (调试用)。
    GetPid = 10,
    /// 打开文件 (lab-9)。`a0` = 路径, `a1` = 模式, 返回 fd。
    Open = 11,
    /// 关闭文件 (lab-9)。
    Close = 12,
    /// 移动读写位置 (lab-9)。
    Lseek = 13,
}

// `Syscall` 的取值个数 + 1 —— 用于在内核里对调用号做范围检查。
pub const SYS_MAX: usize = 14;

impl Syscall {
    // 解码寄存器里的原始调用号。未定义的返回 `None`, 让调用方明确地报错。
    pub const fn from_raw(raw: usize) -> Option<Self> {
        Some(match raw {
            1 => Syscall::Exit,
            2 => Syscall::Fork,
            3 => Syscall::Read,
            4 => Syscall::Write,
            5 => Syscall::Exec,
            6 => Syscall::Wait,
            9 => Syscall::Mmap,
            10 => Syscall::GetPid,
            11 => Syscall::Open,
            12 => Syscall::Close,
            13 => Syscall::Lseek,
            0 => Syscall::HelloWorld,
            _ => return None,
        })
    }
}

// 系统调用返回值: 成功或失败。
// 用 `Result` 而非 C 的"负数表示错误": 忘记处理错误会变成编译警告。
// 这只是内核内部类型; 跨特权级时仍编码成 `isize` (见 encode_ret/decode_ret)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysError {
    /// 参数非法 (空指针、越界、长度为零等)。
    BadArg,
    /// 文件或进程不存在。
    NoEnt,
    /// 内存不足 (物理页分配失败)。
    NoMem,
    /// 底层 I/O 失败。
    Io,
    /// 文件描述符非法或已用尽。
    NoFd,
    /// 可执行文件格式错误。
    BadFmt,
    /// 该功能尚未实现 (教学仓库里大量存在, 用于明确区分
    /// "没实现" 与 "实现错了")。
    NoSys,
}

impl SysError {
    /// 编码成 ABI 上的负数。见 `encode_ret`。
    pub const fn as_raw(self) -> isize {
        match self {
            SysError::BadArg => -1,
            SysError::NoEnt => -2,
            SysError::NoMem => -3,
            SysError::Io => -4,
            SysError::NoFd => -5,
            SysError::BadFmt => -6,
            SysError::NoSys => -38,
        }
    }
}

/// 内核内部使用的返回值类型。
pub type SysResult = Result<usize, SysError>;

/// 把一个 [`SysResult`] 编码成 ABI 上的一个 `isize` (放进 `a0`)。
///
/// 编码规则沿用 Unix 的传统: `[0, isize::MAX]` 是成功的返回值,
/// 负数是 `-errno`。这个"负数区间"的约定不是随意选的 ——
/// 它使得**有效的字节数或地址永远不可能和错误码混淆**,
/// 因为长度和地址都不可能为负。
#[inline]
pub const fn encode_ret(r: SysResult) -> isize {
    match r {
        Ok(v) => v as isize,
        Err(e) => e.as_raw(),
    }
}

/// [`encode_ret`] 的逆运算 (用户态库使用)。
#[inline]
pub const fn decode_ret(raw: isize) -> SysResult {
    if raw < 0 {
        Err(match raw {
            -1 => SysError::BadArg,
            -2 => SysError::NoEnt,
            -3 => SysError::NoMem,
            -4 => SysError::Io,
            -5 => SysError::NoFd,
            -6 => SysError::BadFmt,
            _ => SysError::NoSys,
        })
    } else {
        Ok(raw as usize)
    }
}

/// 标准文件描述符。
pub const STDIN_FILENO: usize = 0;
pub const STDOUT_FILENO: usize = 1;
pub const STDERR_FILENO: usize = 2;

/// `open` 的模式位。
pub mod open_mode {
    pub const RDONLY: usize = 0;
    pub const WRONLY: usize = 1;
    pub const RDWR: usize = 2;
    pub const CREATE: usize = 4;
}

/// 用户程序的入口符号名。
///
/// 用户程序是独立编译的 ELF, 内核在 `exec` 时需要知道"从哪个地址开始
/// 执行"。这个常量是链接脚本模板 `user/arch/<arch>/user.ld.in` 里的 `ENTRY(...)`
/// 与内核 `exec` 实现之间的契约。
pub const USER_ENTRY_SYMBOL: &str = "_user_start";

// ===========================================================================
// 编译期自检
// ===========================================================================
// 这一段是"防止 ABI 被静默改坏"的保险。如果有人调整了 Syscall 的判别值,
// ABI 就变了 —— 但用户程序可能仍然编译通过, 问题要到运行时才暴露。
// 把这些值写死断言一次, 改号会立刻编译失败, 迫使改动者去检查用户态。
const _: () = {
    assert!(Syscall::Exit as usize == 1);
    assert!(Syscall::Fork as usize == 2);
    assert!(Syscall::Read as usize == 3);
    assert!(Syscall::Write as usize == 4);
    assert!(Syscall::Exec as usize == 5);
    assert!(Syscall::Wait as usize == 6);
    assert!(Syscall::Mmap as usize == 9);
    assert!(Syscall::GetPid as usize == 10);
    assert!(Syscall::Open as usize == 11);
    assert!(Syscall::Close as usize == 12);
    assert!(Syscall::Lseek as usize == 13);
    assert!((Syscall::Exit as usize) < SYS_MAX);
};
