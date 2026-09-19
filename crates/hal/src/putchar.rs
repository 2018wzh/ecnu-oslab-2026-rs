//! `hal::putchar` — 内核的统一字符输出出口。
//!
//! 内核有 SBI 控制台 (不依赖 MMIO, 启动最早可用) 与 UART 驱动 (快但需
//! 正确平台常量) 两种打印手段。这里用一个全局可切换后端统一: 启动早期
//! 走 SBI, UART 就绪后切到 UART —— 把"内核跑起来了"和"板级常量对"
//! 分两步验证。后端用 `AtomicU8`, 使多核同时打印时的竞争至少是安全的。

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// 输出后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Backend {
    /// 固件控制台 (SBI)。启动最早可用。
    Sbi = 0,
    /// 16550 UART 驱动。需要 platform 常量正确。
    Uart = 1,
}

/// 当前的输出后端。默认走 SBI (启动最早的默认状态)。
static BACKEND: AtomicU8 = AtomicU8::new(Backend::Sbi as u8);

/// UART 驱动的写函数指针。
///
/// 用函数指针而非直接调 drivers: 依赖方向是 drivers -> hal, 所以 hal
/// **不能**依赖 drivers (会成环)。drivers 初始化时把自己实现注册进来。
static UART_PUTC: AtomicUsize = AtomicUsize::new(0);

/// 注册 UART 的输出函数, 由 UART 驱动初始化完成后调用; 在此之前默认走
/// SBI。调用后输出后端切换为 UART。
///
/// # Safety
/// `f` 必须是"给定任意 `u8` 都能安全完成、不会 panic、不会递归调用
/// [`putc`]"的函数, 否则内核在打印日志时无限递归或 panic。
pub unsafe fn register_uart(f: fn(u8)) {
    UART_PUTC.store(f as usize, Ordering::Release);
    BACKEND.store(Backend::Uart as u8, Ordering::Release);
}

/// 当前的后端。
pub fn backend() -> Backend {
    match BACKEND.load(Ordering::Acquire) {
        0 => Backend::Sbi,
        _ => Backend::Uart,
    }
}

/// 强制指定后端 (调试用)。
pub fn set_backend(b: Backend) {
    BACKEND.store(b as u8, Ordering::Release);
}

/// 输出一个字节。
///
/// 内核里**唯一**的字符输出出口, 所有 `print!`/`println!` 最终落在这里。
#[inline]
pub fn putc(c: u8) {
    match backend() {
        Backend::Sbi => crate::arch::sbi::console_putchar(c),
        Backend::Uart => {
            let f = UART_PUTC.load(Ordering::Acquire);
            if f == 0 {
                // 后端说走 UART 却没注册函数 (只有手工 `set_backend(Uart)`
                // 才会这样) —— 退回 SBI, 不在打印路径上 panic。
                crate::arch::sbi::console_putchar(c);
            } else {
                // SAFETY: 由 `register_uart` 契约保证指针是合法 `fn(u8)`;
                // `AtomicUsize` 只跨核传值, 不改变函数生命周期 (永远 'static)。
                let f: fn(u8) = unsafe { core::mem::transmute::<usize, fn(u8)>(f) };
                f(c);
            }
        }
    }
}

/// 输出一个字符串。
///
/// 不做 `\n` -> `\r\n` 转换: 那是**驱动**的职责 (16550 与 SBI 各有处理),
/// 放在这层会让"字符串长度"与"实际发字节数"不一致。
pub fn puts(s: &str) {
    for b in s.bytes() {
        putc(b);
    }
}

/// 输出一个字节切片 (可能不是合法 UTF-8)。
pub fn putbytes(bs: &[u8]) {
    for b in bs {
        putc(*b);
    }
}
