//! 通用用户调用接口；寄存器约定由 arch 层处理。
/// # Safety
/// 调用者必须满足具体调用的指针、内存修改及生命周期契约。
pub unsafe fn syscall6(number: usize, args: [usize; 6]) -> isize {
    // SAFETY: 将调用者保证的契约传递给所选架构。
    unsafe { crate::arch::syscall6(number, args) }
}
pub fn hello() -> isize {
    // SAFETY: hello 无指针参数，也不改变调用者的地址空间。
    unsafe { crate::arch::syscall6(oslab_uapi::SYS_HELLO, [0; 6]) }
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
