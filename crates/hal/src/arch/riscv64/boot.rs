//! 汇编建立栈后进入 Rust。
use crate::platform::{HART_FIRST, NCPU};
pub const STACK_SHIFT: usize = 14;
pub const STACK_SIZE: usize = 1 << STACK_SHIFT;
core::arch::global_asm!(include_str!("entry.S"),
    hart_first = const HART_FIRST, hart_end = const HART_FIRST + NCPU,
    stack_shift = const STACK_SHIFT, stack_bytes = const STACK_SIZE * NCPU);

// TODO(lab-1): 整理本核初始状态，再调用 kernel_main。
#[unsafe(no_mangle)]
pub extern "C" fn start(_hartid: usize, _opaque: usize) -> ! {
    todo!("lab-1: start")
}

unsafe extern "C" { pub fn kernel_main() -> !; }
