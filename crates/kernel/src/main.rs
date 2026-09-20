#![no_std]
#![no_main]
// 教学框架的接口在学生完成任务前允许暂未使用。
#![allow(dead_code)]
mod console;
mod console_input;
mod elf;
mod print;
mod panic;
mod lock;
mod mem;
mod trap;
mod proc;
mod syscall;
mod fs;
use oslab_hal as _;

// TODO(lab-3): 承接前序启动流程，接入共享 trap 初始化与每核 trap 初始化。
// TODO(lab-1): 主核初始化并启动其他核，以原子操作同步；每核打印一次。
// TODO(lab-5): 创建首进程前初始化 mmap 节点池。
// TODO(lab-6): 主核初始化进程表后完成内存、每核分页与中断初始化后调用 proc::make_first；所有核完成各自初始化与发布同步后进入调度器，永不返回。
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    todo!("lab-1: kernel_main")
}

// TODO(lab-7): 主核 fs::block::init，在磁盘 PLIC 使能之前完成；失败停止。
