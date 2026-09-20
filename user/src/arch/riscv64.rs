//! RISC-V 用户系统调用 ABI。
/// # Safety
/// 调用者须满足具体调用的指针有效性、生命周期及内存修改约定。
pub unsafe fn syscall6(number: usize, args: [usize; 6]) -> isize {
    let result;
    // SAFETY: 调用者保证具体系统调用契约；内核按 ABI 恢复除返回值外的用户寄存器。
    unsafe {
        core::arch::asm!("ecall", inlateout("a0") args[0] => result,
            in("a1") args[1], in("a2") args[2], in("a3") args[3],
            in("a4") args[4], in("a5") args[5], in("a7") number);
    }
    result
}
