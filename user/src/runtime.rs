//! `oslab_user` — 用户态运行时 (无依赖): 只有系统调用包装与打印。
//! 用户程序不能调内核函数 (U-mode 下不可执行), 只能 `ecall` 陷入 S-mode,
//! 所以由本层把"参数放进 a0-a5、调用号放进 a7、执行 ecall"包装成 Rust 函数。
//! `#![no_std]`: 无 Vec/String/堆分配 (内核在 lab-9 前没有这些)。

// 本文件是 `oslab_user` crate 的一个模块 (见 lib.rs 的 `mod runtime;`)。
// `#![no_std]` 写在 crate 根 (lib.rs)。
// 宏 (`println` / `print` / `entry`) 用 `$crate::` 引用本 crate 根名字,
// 因为 `#[macro_export]` 让它们被调用方 (用户程序) 的 `use oslab_user::*`
// 引入, 需经调用方 crate 路径解析。

use core::panic::PanicInfo;

// ===========================================================================
// ABI 定义 (与 crates/uapi 和 docs/abi-spec.md 必须一致)
// ---------------------------------------------------------------------------
// 用户态运行时保持零依赖 (不 `use oslab_uapi`), 故这里保留一份 ABI 数值
// 副本。防漂移的三道防线:
// uapi 有同名定义 + 编译期断言 + xtask 架构规则检查。
// ===========================================================================

/// 系统调用号 (与 `crates/uapi` 的 `Syscall` 判别值一一对应)。
#[repr(usize)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Syscall {
    HelloWorld = 0,
    Exit = 1,
    Fork = 2,
    Read = 3,
    Write = 4,
    Exec = 5,
    Wait = 6,
    Sleep = 7,
    Mmap = 9,
    GetPid = 10,
    Open = 11,
    Close = 12,
    Lseek = 13,
}

/// 系统调用错误码 (ABI 上是负数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysError {
    BadArg,
    NoEnt,
    NoMem,
    Io,
    NoFd,
    BadFmt,
    NoSys,
}

impl SysError {
    /// 编码成 ABI 上的负数。
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

/// 把 `a0` 里的原始返回值解码成 `Result`。
pub const fn decode_ret(raw: isize) -> Result<usize, SysError> {
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

/// 发起一次系统调用, 返回内核给的原始值 (负数 = 错误)。
///
/// ABI (见 `docs/abi-spec.md`): a7=调用号, a0..a2=参数, 陷入后 a0=返回值
/// (负数表示错误)。具体机制 (`ecall` / 寄存器约定) 是架构相关的,
/// 委托给 [`crate::arch`] (见 `arch/mod.rs`) 的 [`crate::arch::syscall_raw`]。
///
/// 公开是为了调试 ABI: 上层把返回值解码成 `Result`, 而这里能看到原始值,
/// 以区分"内核返回了错误码"与"我们解码错了"。
#[inline(always)]
pub fn syscall_raw(num: usize, a0: usize, a1: usize, a2: usize) -> isize {
    crate::arch::syscall_raw(num, a0, a1, a2)
}

/// 发起系统调用并把返回值解码成 `Result`。
pub fn syscall(call: Syscall, a0: usize, a1: usize, a2: usize) -> Result<usize, SysError> {
    let raw = syscall_raw(call as usize, a0, a1, a2);
    decode_ret(raw)
}

// ---------------------------------------------------------------------------
// 系统调用封装
// ---------------------------------------------------------------------------
// 每个封装都是"把 Rust 的类型变成 ABI 上的整数"这一件事。它们存在的
// 价值是: 用户程序不必记住调用号和参数顺序 —— 那是 ABI 的细节,
// 应该只在一个地方知道。

/// (lab-4..8) 内核打印固定字符串。
pub fn helloworld() -> Result<usize, SysError> {
    syscall(Syscall::HelloWorld, 0, 0, 0)
}

/// 写: `write(fd, buf, len)`。
/// (lab-6) 让当前进程睡 n 个 tick。
pub fn sleep(ticks: usize) -> Result<usize, SysError> {
    syscall(Syscall::Sleep, ticks, 0, 0)
}

pub fn write(fd: usize, buf: &[u8]) -> Result<usize, SysError> {
    syscall(Syscall::Write, fd, buf.as_ptr() as usize, buf.len())
}

/// 读: `read(fd, buf, len)`。
pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, SysError> {
    syscall(Syscall::Read, fd, buf.as_mut_ptr() as usize, buf.len())
}

/// 结束当前进程。不返回。
pub fn exit(code: usize) -> ! {
    // 显式丢弃返回值: 调用不返回, 无需处理成败。
    let _ = syscall(Syscall::Exit, code, 0, 0);
    // 内核 exit 不会返回, 但返回类型是整数, 编译器不知"此路径不返回"。
    // 用死循环而非 unreachable_unchecked: 万一内核有 bug 真返回了,
    // 用户程序停在明确的死循环里 (gdb 能看到 PC 卡在哪)。
    loop {
        core::hint::spin_loop();
    }
}

/// 取当前进程号。
pub fn getpid() -> Result<usize, SysError> {
    syscall(Syscall::GetPid, 0, 0, 0)
}

/// 打开文件, 返回文件描述符。
///
/// # 注意: `path` 必须以 `\0` 结尾
///
/// 内核按 C 约定读路径 (读到 `\0` 为止)。Rust 的 `b"..."` 字节串不会自动
/// 补 `\0`, 调用时请写 `b"/init\0"`。忘了的话 `open` 返回 `BadArg`。
pub fn open(path: &[u8], mode: usize) -> Result<usize, SysError> {
    syscall(Syscall::Open, path.as_ptr() as usize, mode, 0)
}

/// 关闭文件描述符。
pub fn close(fd: usize) -> Result<usize, SysError> {
    syscall(Syscall::Close, fd, 0, 0)
}

/// 用新映像替换当前进程。成功不返回。
pub fn exec(path: &[u8]) -> Result<usize, SysError> {
    syscall(Syscall::Exec, path.as_ptr() as usize, 0, 0)
}

/// 等待子进程退出。
pub fn wait() -> Result<usize, SysError> {
    syscall(Syscall::Wait, 0, 0, 0)
}

/// 等待一个子进程退出, 并把它的退出状态写进 `status`。
///
/// 返回值是子进程号, 退出状态经指针写回 (与 POSIX `waitpid` 相近)。
pub fn wait_status(status: &mut usize) -> Result<usize, SysError> {
    syscall(Syscall::Wait, status as *mut usize as usize, 0, 0)
}

/// 移动文件读写位置。`whence` 只支持 0 (SEEK_SET)。
///
/// 内核只实现了 SEEK_SET, 实验需要"回到文件开头重读"这一个能力。
pub fn lseek(fd: usize, offset: usize) -> Result<usize, SysError> {
    syscall(Syscall::Lseek, fd, offset, 0)
}

/// 创建子进程。父进程得到子进程号, 子进程得到 0。
pub fn fork() -> Result<usize, SysError> {
    syscall(Syscall::Fork, 0, 0, 0)
}

// ---------------------------------------------------------------------------
// 极小的内存函数 (编译器会自己调用它们)
// ---------------------------------------------------------------------------
// 用户程序是 `no_std` + 无 core 库的裸二进制。一些平常写法 (数组零初始化、
// 大结构体赋值) 会让 rustc 生成
// 对 memset/memcpy 等的调用, 不实现就会链接期报 undefined reference。
// 实现刻意保持简洁。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        // SAFETY: 调用者保证 [dst, dst+n) 是可写的 (这是 C 的约定)。
        unsafe { *dst.add(i) = c as u8 };
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        // SAFETY: 调用者保证两侧区间合法且不重叠。
        unsafe { *dst.add(i) = *src.add(i) };
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dst as usize) < (src as usize) {
        return unsafe { memcpy(dst, src, n) };
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        // SAFETY: 同上; 从后往前拷使得重叠区间也是安全的。
        unsafe { *dst.add(i) = *src.add(i) };
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        // SAFETY: 调用者保证两侧区间各 n 字节可读。
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

// ---------------------------------------------------------------------------
// 输出
// ---------------------------------------------------------------------------

/// 把一个字符串写到标准输出, 错误静默忽略 (报告输出失败本身也需要
/// 输出, 会递归)。关心错误时请直接用 [`write`]。
///
/// 名为 `print_str` 而非 `print` 是为了避开下面 `print!` 宏的同名遮蔽。
pub fn print_str(s: &str) {
    let _ = write(STDOUT_FILENO, s.as_bytes());
}

/// 与 [`print`] 相同, 但输出到标准错误。
pub fn eprint(s: &str) {
    let _ = write(STDERR_FILENO, s.as_bytes());
}

/// 输出一个无符号整数 (十进制)。
///
/// 手写整数转字符串而非 `core::fmt`: 后者会给每个用户程序带进几 KB
/// 格式化机器, 而程序总共才几百字节。
pub fn print_dec(mut v: usize) {
    if v == 0 {
        print_str("0");
        return;
    }
    // 十进制 64 位最多 20 位数字。
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        // `buf[i]` 会插入边界检查, 失败要调 panic 基础设施 (而用户程序
        // 没有 panic 处理器, 会拉入未定义符号)。用 `get_mut` 把越界显式
        // 处理成停止, 不依赖优化器。
        match buf.get_mut(i) {
            Some(slot) => *slot = b'0' + (v % 10) as u8,
            None => break,
        }
        v /= 10;
    }
    // `&buf[i..]` 同样会插边界检查, 用 `get` 拿 Option, 越界时什么都不输出。
    if let Some(out) = buf.get(i..) {
        let _ = write(STDOUT_FILENO, out);
    }
}

/// 输出一个十六进制整数 (无前导零)。
pub fn print_hex(mut v: usize) {
    if v == 0 {
        print_str("0x0");
        return;
    }
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        let d = (v & 0xf) as u8;
        let c = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        // 同上: 用 get_mut 避免拉进 panic_bounds_check。
        match buf.get_mut(i) {
            Some(slot) => *slot = c,
            None => break,
        }
        v >>= 4;
    }
    print_str("0x");
    if let Some(out) = buf.get(i..) {
        let _ = write(STDOUT_FILENO, out);
    }
}

// ---------------------------------------------------------------------------
// 极简格式化输出
// ---------------------------------------------------------------------------

/// 格式化参数。内部类型, 由 [`println!`] 宏使用。
pub enum Arg<'a> {
    Str(&'a str),
    Dec(usize),
    Hex(usize),
    Char(u8),
}

/// 按顺序输出一组格式化参数。
///
/// 支持的格式符: `{}` (十进制) `{:x}` (十六进制) `{:c}` (字符)
/// `{:s}` (字符串)。不实现完整 `core::fmt`, 用户程序够用即可。
pub fn fmt(args: &[Arg]) {
    let mut it = args.iter();
    // 第一个参数是格式串, 其余按顺序填充 `{}`。
    let Some(Arg::Str(fmtstr)) = it.next() else {
        return;
    };

    let bytes = fmtstr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            // 攒一段连续的非格式文本批量 write —— 每次 write 是一次 ecall,
            // 逐字节发长字符串会慢得多。
            let start = i;
            while i < bytes.len() && bytes[i] != b'{' {
                i += 1;
            }
            // 用 `get` 而非 `&bytes[start..i]` 切片: 切片会插越界检查,
            // 失败需要 core::panicking, 而用户程序是 no_std 且无 panic 处理器,
            // 链接会失败且是否触发取决于优化器。用 `get` 让行为不依赖优化器。
            if let Some(seg) = bytes.get(start..i) {
                let _ = write(STDOUT_FILENO, seg);
            }
            continue;
        }

        // ---- 遇到 '{' ----
        // `{{` 是转义的左花括号。
        if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let _ = write(STDOUT_FILENO, b"{");
            i += 2;
            continue;
        }

        // 解析可选的 `:x` 说明符, 然后跳过 '}'。
        let mut i2 = i + 1;
        let mut is_hex = false;
        if i2 < bytes.len() && bytes[i2] == b':' {
            i2 += 1;
            if i2 < bytes.len() && bytes[i2] == b'x' {
                is_hex = true;
                i2 += 1;
            }
        }
        let mut closed = false;
        while i2 < bytes.len() {
            if bytes[i2] == b'}' {
                closed = true;
                i2 += 1;
                break;
            }
            i2 += 1;
        }
        i = i2;

        if !closed {
            // 格式串里有未闭合的 '{' —— 原样输出, 便于发现笔误。
            let _ = write(STDOUT_FILENO, b"{");
            continue;
        }

        // 取下一个参数填进去。
        match it.next() {
            Some(Arg::Str(s)) => print_str(s),
            Some(Arg::Dec(v)) => {
                if is_hex {
                    print_hex(*v);
                } else {
                    print_dec(*v);
                }
            }
            Some(Arg::Hex(v)) => print_hex(*v),
            Some(Arg::Char(c)) => {
                let b = [*c];
                let _ = write(STDOUT_FILENO, &b);
            }
            None => print_str("{?}"),
        }
    }
}

/// 格式化输出到标准输出 (自动换行)。
///
/// 用法: `println!("value = {}", x)` / `println!("hex = {:x}", x)`。
///
/// 宏用 `$crate::` 引用本 crate 根名字 (`print_str` / `fmt` / `Arg` /
/// `arg_to_arg`) —— 它们在 crate 根可见 (lib.rs `pub use runtime::*`)。
/// 用 `$crate::` 而非裸路径, 才能保证宏在**调用方 crate** 里展开时
/// 找到的是本运行时的名字。
#[macro_export]
macro_rules! println {
    () => { $crate::print_str("\n") };
    ($fmt:expr) => {{
        $crate::print_str($fmt);
        $crate::print_str("\n");
    }};
    ($fmt:expr, $($arg:expr),+ $(,)?) => {{
        $crate::fmt(&[$crate::Arg::Str($fmt), $($crate::arg_to_arg($arg)),+]);
        $crate::print_str("\n");
    }};
}

/// 格式化输出到标准输出 (不换行)。
#[macro_export]
macro_rules! print {
    ($fmt:expr) => { $crate::print_str($fmt) };
    ($fmt:expr, $($arg:expr),+ $(,)?) => {{
        $crate::fmt(&[$crate::Arg::Str($fmt), $($crate::arg_to_arg($arg)),+]);
    }};
}

/// 把常见的 Rust 类型转成 [`Arg`]。
///
/// 这是让 `println!("{}", x)` 能工作的胶水。Rust 没有隐式转换,
/// 所以需要为每个受支持的类型写一条规则。
pub fn arg_to_arg<T: Into<ArgOwned>>(v: T) -> Arg<'static> {
    v.into().into_arg()
}

/// 拥有所有权的参数载体 (为了绕开 `Arg<'a>` 的生命周期)。
///
/// `println!` 的参数生命周期长短不一 (`&'static str` vs 局部变量),
/// 无法统一进一个 `Arg<'a>` 数组; 用一个 `'static` 中间类型承接再转换。
pub enum ArgOwned {
    Str(&'static str),
    Dec(usize),
    Hex(usize),
    Char(u8),
}

impl ArgOwned {
    fn into_arg(self) -> Arg<'static> {
        match self {
            ArgOwned::Str(s) => Arg::Str(s),
            ArgOwned::Dec(v) => Arg::Dec(v),
            ArgOwned::Hex(v) => Arg::Hex(v),
            ArgOwned::Char(c) => Arg::Char(c),
        }
    }
}

impl From<&'static str> for ArgOwned {
    fn from(s: &'static str) -> Self {
        ArgOwned::Str(s)
    }
}

macro_rules! arg_from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for ArgOwned {
            fn from(v: $t) -> Self { ArgOwned::Dec(v as usize) }
        }
    )*};
}
arg_from_int!(usize, u32, u16, u8, isize, i32, i16, i8);

// ---------------------------------------------------------------------------
// panic 处理
// ---------------------------------------------------------------------------

/// 用户程序的 panic 处理。
///
/// 与内核 panic 不同: 用户程序 panic 不该让整个系统停下, 只是这一个进程
/// 的问题。这里打印一条信息, 然后 `exit` 让内核回收自己。
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    eprint("[user] panic: ");
    if let Some(loc) = info.location() {
        eprint(loc.file());
        eprint(":");
        print_dec(loc.line() as usize);
    } else {
        eprint("(unknown location)");
    }
    eprint("\n");
    exit(127)
}

/// 用户程序的入口包装。
///
/// 导出 `_user_start` 符号 (内核 `exec` 跳转的地址, 见 `uapi::USER_ENTRY_SYMBOL`),
/// 调用用户的 `main` 后用 `exit` 结束进程。没有 C 运行的 `_start` 负责
/// 退出, 故需此宏兜底。
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _user_start() -> ! {
            $main();
            $crate::exit(0)
        }
    };
}
