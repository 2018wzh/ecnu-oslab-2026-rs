#![no_std]
#![no_main]
// 教学框架的接口在学生完成任务前允许暂未使用。
#![allow(dead_code)]
mod console;
mod print;
mod panic;
mod lock;
mod mem;
use oslab_hal as _;

// TODO(lab-1): 主核初始化并启动其他核，以原子操作同步；每核打印一次。
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    todo!("lab-1: kernel_main")
}
