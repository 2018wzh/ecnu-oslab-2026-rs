//! 串口驱动。目前只有 16550 兼容的 [`uart16550`], 两个平台都用它,
//! 一份源码服务两台机器, 差别只在 platform 常量里。

pub mod uart16550;

pub use uart16550::{Uart16550, UartError};

/// 初始化控制台串口, 并把它注册为内核的输出后端。
///
/// 这同时是"platform 常量正确性"的验证点: 地址或时钟错了, 打印不会有
/// 输出。返回初始化好的驱动实例, 供后续 (例如中断模式) 使用。
pub fn init_console() -> Uart16550 {
    let uart = Uart16550::init(&oslab_hal::platform::PLATFORM);
    // 把驱动的输出函数注册进 hal, 此后内核的 `println!` 走串口硬件。
    //
    // SAFETY: `Uart16550::putc_raw` 不会 panic、不会分配、不依赖未初始化
    // 状态, 满足 `register_uart` 的契约。
    unsafe {
        oslab_hal::putchar::register_uart(Uart16550::putc_raw);
    }
    uart
}
