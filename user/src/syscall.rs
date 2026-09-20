//! 通用用户调用接口；寄存器约定由 arch 层处理。
/// # Safety
/// 调用者必须满足具体调用的指针、内存修改及生命周期契约。
pub unsafe fn syscall6(number: usize, args: [usize; 6]) -> isize {
    // SAFETY: 将调用者保证的契约传递给所选架构。
    unsafe { crate::arch::syscall6(number, args) }
}
/// # Safety
/// 调整后不保留指向已释放堆页的引用；调用者管理堆对象的存活期。
pub unsafe fn brk(top: usize) -> isize {
    // SAFETY: 继承调用者的堆所有权约定。
    unsafe { syscall6(oslab_uapi::SYS_BRK, [top, 0, 0, 0, 0, 0]) }
}
/// # Safety
/// 调用者管理新区域，检查返回值后才访问；不得与存活对象重叠。
pub unsafe fn mmap(address: usize, len: usize) -> isize {
    // SAFETY: 继承调用者的地址空间约定。
    unsafe { syscall6(oslab_uapi::SYS_MMAP, [address, len, 0, 0, 0, 0]) }
}
/// # Safety
/// 解除前结束区域内所有引用和访问；返回后不得继续使用原地址。
pub unsafe fn munmap(address: usize, len: usize) -> isize {
    // SAFETY: 继承调用者的独占访问约定。
    unsafe { syscall6(oslab_uapi::SYS_MUNMAP, [address, len, 0, 0, 0, 0]) }
}

/// # Safety
/// str 指向可读、以 NUL 结尾且调用期间存活的用户字符串。
pub unsafe fn print_str(str: *const u8) -> isize {
    // SAFETY: 调用者保证字符串有效。
    unsafe { syscall6(oslab_uapi::SYS_PRINT_STR, [str as usize, 0, 0, 0, 0, 0]) }
}
pub fn print_int(value: i32) -> isize {
    // SAFETY: 纯值参数。
    unsafe { syscall6(oslab_uapi::SYS_PRINT_INT, [value as usize, 0, 0, 0, 0, 0]) }
}
pub fn getpid() -> isize {
    // SAFETY: 无指针参数。
    unsafe { syscall6(oslab_uapi::SYS_GETPID, [0; 6]) }
}
/// # Safety
/// 调用点必须允许单线程地址空间复制；子进程不继承可跨进程共享的 Rust 所有权。
pub unsafe fn fork() -> isize {
    // SAFETY: 调用者允许分叉执行。
    unsafe { syscall6(oslab_uapi::SYS_FORK, [0; 6]) }
}
/// # Safety
/// status 为零或有效且独占可写的 i32 用户地址。
pub unsafe fn wait(status: *mut i32) -> isize {
    // SAFETY: 调用者保证状态缓冲有效；零表示忽略状态。
    unsafe { syscall6(oslab_uapi::SYS_WAIT, [status as usize, 0, 0, 0, 0, 0]) }
}
pub fn exit(status: i32) -> ! {
    // SAFETY: 终止当前进程，纯值参数。
    unsafe { syscall6(oslab_uapi::SYS_EXIT, [status as usize, 0, 0, 0, 0, 0]); }
    loop { core::hint::spin_loop(); }
}
pub fn sleep(ticks: usize) -> isize {
    // SAFETY: 纯值参数。
    unsafe { syscall6(oslab_uapi::SYS_SLEEP, [ticks, 0, 0, 0, 0, 0]) }
}
