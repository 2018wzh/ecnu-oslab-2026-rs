//! 串口与紧急输出。
use oslab_drivers::serial::Uart;
use oslab_hal::{platform, arch::sbi};
// SAFETY: 使用所选平台的固定 UART MMIO 地址，lab-1 分页关闭。
static UART: Uart = unsafe { Uart::new(platform::UART_BASE, platform::UART_SHIFT) };
pub fn init() { UART.init(platform::UART_CLOCK); }
pub fn putc(c: u8) { if c == b'\n' { UART.putc(b'\r'); } UART.putc(c); }
pub fn emergency(s: &str) {
    for c in s.bytes() { if c == b'\n' { sbi::putc(b'\r'); } sbi::putc(c); }
}
