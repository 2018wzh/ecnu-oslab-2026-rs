mod sysfunc;
mod memory;
mod process;
mod disk;
use oslab_hal::arch::Syscall;
// 教师分派：完整 usize 编号，0 不合法，不能先截断。
const HANDLERS: [fn(&Syscall) -> isize; 21] = [
    memory::brk, memory::mmap, memory::munmap, sysfunc::print_str, sysfunc::print_int,
    |_| process::getpid(), |_| process::fork(), process::wait, process::exit, process::sleep,
    disk::alloc_block, disk::free_block, disk::alloc_inode, disk::free_inode, disk::show_bitmap, disk::get_block, disk::read_block, disk::write_block, disk::put_block, disk::show_buffer, disk::flush_buffer,
];
pub fn dispatch(call: &Syscall) -> isize {
    let handler = call.number.checked_sub(1).and_then(|n| HANDLERS.get(n));
    let Some(handler) = handler else {
        // SAFETY: 用户 trap 期间 current 存活，只复制 pid，不保留进程借用。
        let pid = unsafe { (*crate::proc::current()).pid };
        panic!("unknown syscall {} pid={}", call.number, pid);
    };
    handler(call)
}
