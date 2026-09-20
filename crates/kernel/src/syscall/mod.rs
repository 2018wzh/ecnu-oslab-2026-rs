mod sysfunc;
mod memory;
use oslab_hal::arch::Syscall;
// 与 uapi 的 0..=6 对应；保留完整 usize 编号进行边界检查。
const HANDLERS: [fn(&Syscall) -> isize; 7] = [
    |_| sysfunc::hello(), memory::test_copyin, memory::test_copyout,
    memory::test_copyinstr, memory::brk, memory::mmap, memory::munmap,
];
pub fn dispatch(call: &Syscall) -> isize {
    let Some(handler) = HANDLERS.get(call.number) else {
        // SAFETY: 用户 trap 调用期间 current 指向存活进程；只复制 pid，不保留借用。
        let pid = unsafe { (*crate::proc::current()).pid };
        panic!("unknown syscall {} pid={}", call.number, pid);
    };
    handler(call)
}
