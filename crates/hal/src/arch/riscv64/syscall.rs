//! 用户 trapframe 与通用请求之间的转换。
use crate::arch::Syscall;
use super::trap::TrapFrame;
pub fn decode(frame: &TrapFrame) -> Syscall {
    Syscall { number: frame.x[17], args: core::array::from_fn(|i| frame.x[10 + i]) }
}
pub fn return_value(frame: &mut TrapFrame, result: isize) {
    frame.x[10] = result as usize;
    frame.epc += 4;
}
