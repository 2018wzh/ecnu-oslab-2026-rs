//! 通用用户调用接口；寄存器约定由 arch 层处理。
pub fn hello() -> isize {
    // SAFETY: hello 无指针参数，也不改变调用者的地址空间。
    unsafe { crate::arch::syscall6(oslab_uapi::SYS_HELLO, [0; 6]) }
}
