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

/// exec_success 由原调用号和结果判定；新 PC 已由 exec 安装，不能再推进。
pub fn finish(frame: &mut super::trap::TrapFrame, result: isize, exec_success: bool) {
    if exec_success { frame.x[10] = result as usize; }
    else { return_value(frame, result); }
}
