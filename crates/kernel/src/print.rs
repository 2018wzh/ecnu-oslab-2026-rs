use core::fmt::{self, Write};
use crate::lock::SpinLock;
pub static PRINT_LOCK: SpinLock = SpinLock::UNINIT;
pub struct Writer;
impl Write for Writer {
    // TODO(lab-1): 将字符串的字节交给 console::putc。
    fn write_str(&mut self, _s: &str) -> fmt::Result { todo!("lab-1: Writer::write_str") }
}
pub fn init() {
    crate::console::init();
    // SAFETY: 主核在启动其他 CPU 前调用一次。
    unsafe { PRINT_LOCK.init(); }
}
// TODO(lab-1): 在同一守卫生命周期内完成整次格式化输出。
pub fn print(_args: fmt::Arguments<'_>) { todo!("lab-1: print") }
#[macro_export]
macro_rules! print { ($($arg:tt)*) => { $crate::print::print(format_args!($($arg)*)) }; }
#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)) };
}
