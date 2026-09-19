//! 内核格式化输出。分层:
//!   console (这里)  格式化: 数字/字符串/对齐 -> 字节流
//!   hal::putchar    路由: 决定走 SBI 还是 UART
//!   drivers::serial 硬件: 操作 16550 寄存器
//! 这里不含任何 UART 知识, 换串口硬件不用改这里的格式化代码。
//!
//! 不引入 `core::fmt` 全套 (体积换不来灵活性), 但提供一个最小的
//! `Writer` 适配器让 `write!` 可用; `print!`/`println!` 走轻量路径。
//! 打印不加锁: `print!` 会在 trap/panic 里被调用, 持锁可能死锁;
//! 多核下允许输出交错 (能交错远好于死锁)。需要整行原子输出时用
//! [`with_lock`]。

use core::sync::atomic::{AtomicBool, Ordering};

use oslab_hal::putchar;

/// 一个极简自旋锁, 只用于串口输出。
///
/// 保护资源是单一、全局的且必须在任意上下文 (含 panic) 里获取, 用
/// `AtomicBool` + 自旋而非返回守卫的通用 `Mutex`, 避免 panic 路径上
/// 重复加锁死锁。
pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    /// 构造一个未加锁的自旋锁。
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    /// 获取锁, 返回一个 RAII 守卫。
    pub fn lock(&self) -> SpinGuard<'_> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinGuard { lock: self }
    }

    /// 尝试获取锁, 不等待。
    pub fn try_lock(&self) -> Option<SpinGuard<'_>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(SpinGuard { lock: self })
        } else {
            None
        }
    }
}

impl Default for SpinLock {
    fn default() -> Self {
        Self::new()
    }
}

/// [`SpinLock::lock`] 返回的守卫, 被丢弃时自动解锁。
pub struct SpinGuard<'a> {
    lock: &'a SpinLock,
}

impl Drop for SpinGuard<'_> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

// 全局打印锁。
static PRINT_LOCK: SpinLock = SpinLock::new();

/// 在持有打印锁的情况下执行 `f` (保证一整行输出不被其他核打断)。
pub fn with_lock<R>(f: impl FnOnce() -> R) -> R {
    let _g = PRINT_LOCK.lock();
    f()
}

// ===========================================================================
// 数字输出
// ===========================================================================

/// 以小写十六进制打印一个指针/地址, 带 `0x` 前缀。
pub fn print_hex(v: usize) {
    putchar::puts("0x");
    print_hex_bare(v);
}

/// 不带前缀的十六进制。
pub fn print_hex_bare(mut v: usize) { }

/// 以十进制打印一个无符号数。
pub fn print_dec(mut v: usize) { }

/// 以 MiB 为单位打印一个字节数。
///
/// 不是整数 MiB 时打印原始字节数, 便于看出平台常量写错
/// (如 128*1000*1000 而非 128*1024*1024)。
pub fn print_size_mib(bytes: usize) {
    const MIB: usize = 1024 * 1024;
    if bytes % MIB == 0 {
        print_dec(bytes / MIB);
        putchar::puts(" MiB");
    } else {
        print_dec(bytes);
        putchar::puts(" B");
    }
}

/// 以小写十六进制打印一个 32 位值 (用于设备寄存器)。
pub fn print_hex32(v: u32) {
    print_hex(v as usize);
}

// ===========================================================================
// `core::fmt::Write` 适配器
// ===========================================================================
// 面向熟悉标准库风格的学生: use core::fmt::Write; write!(w, "...", ...) 。

/// 把 `core::fmt` 的输出接到 [`oslab_hal::putchar`] 上。
pub struct Writer;

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        putchar::puts(s);
        Ok(())
    }
}

/// 用 `core::fmt` 格式化到串口。
pub fn write_fmt(args: core::fmt::Arguments) -> core::fmt::Result {
    use core::fmt::Write;
    Writer.write_fmt(args)
}

// ===========================================================================
// 宏
// ===========================================================================

/// 打印到内核控制台 (不加换行)。
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let _ = $crate::console::write_fmt(format_args!($($arg)*));
    }};
}

/// 打印到内核控制台, 并加上换行。
#[macro_export]
macro_rules! println {
    () => {{
        $crate::print!("\n");
    }};
    ($($arg:tt)*) => {{
        let _ = $crate::console::write_fmt(format_args!("{}\n", format_args!($($arg)*)));
    }};
}