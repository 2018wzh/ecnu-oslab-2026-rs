mod memory;
mod process;
mod file;
use oslab_hal::arch::Syscall;
// 教师前八项，学生补齐 9～22 接线；模块中已提供全部函数声明/任务占位。
const HANDLERS: [Option<fn(&Syscall) -> isize>; 22] = [
    Some(memory::brk), Some(memory::mmap), Some(memory::munmap),
    Some(|_| process::fork()), Some(process::wait), Some(process::exit),
    Some(process::sleep), Some(|_| process::getpid()),
    // TODO(lab-9): 用服务函数替换下列 14 个 None。
    None, None, None, None, None, None, None, None, None, None, None, None, None, None,
];
pub fn dispatch(call: &Syscall) -> isize {
    let handler = call.number.checked_sub(1).and_then(|n| HANDLERS.get(n)).copied().flatten();
    let Some(handler) = handler else {
        // SAFETY: 用户 trap 期间 current 存活，只复制 pid，不保留进程借用。
        let pid = unsafe { (*crate::proc::current()).pid };
        panic!("unknown syscall {} pid={}", call.number, pid);
    };
    handler(call)
}
/// 教师参数辅助：保留用户地址为整数，经页表复制后查找终止符。
/// 返回不含 NUL 的字节长度；127 字节内容另加 NUL。
pub fn arg_path(call: &Syscall, n: usize, out: &mut [u8; 128]) -> Result<usize, ()> {
    let addr = *call.args.get(n).ok_or(())?;
    // SAFETY: 当前进程在调用期间存活；仅共享借用进行用户复制。
    unsafe { crate::mem::uvm::copy_str_from_user(&*crate::proc::current(), out, addr); }
    out.iter().position(|b| *b == 0).ok_or(())
}
