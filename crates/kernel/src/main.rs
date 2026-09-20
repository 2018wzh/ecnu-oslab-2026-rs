#![no_std]
#![no_main]
// 教学框架的接口在学生完成任务前允许暂未使用。
#![allow(dead_code)]
mod console;
mod print;
mod panic;
mod lock;
mod mem;
mod trap;
mod proc;
use oslab_hal as _;

// TODO(lab-3): 承接前序启动流程，接入共享 trap 初始化与每核 trap 初始化。
// TODO(lab-1): 主核初始化并启动其他核，以原子操作同步；每核打印一次。
// TODO(lab-4): 主核完成内存、每核分页与中断初始化后调用 proc::make_first；其他核不进入首进程。
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    todo!("lab-1: kernel_main")
}
