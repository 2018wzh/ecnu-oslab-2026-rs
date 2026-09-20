//! 当前阶段所需的寄存器操作。
pub fn irq_enabled() -> bool {
    let x: usize;
    // SAFETY: 读取 S-mode 可访问的 sstatus。
    unsafe { core::arch::asm!("csrr {}, sstatus", out(reg) x); }
    x & 2 != 0
}
pub fn irq_disable() {
    // SAFETY: 只关闭当前 hart 的中断。
    unsafe { core::arch::asm!("csrci sstatus, 2"); }
}
pub fn irq_enable() {
    // SAFETY: 调用方已安装可用的中断入口。
    unsafe { core::arch::asm!("csrsi sstatus, 2"); }
}
pub fn early_init() {
    unsafe extern "C" { fn early_trap(); }
    // SAFETY: 仅在分页关闭的启动阶段调用；停车入口不访问栈。
    unsafe { core::arch::asm!("csrci sstatus, 2", "csrw sie, zero", "csrw satp, zero", "csrw stvec, {}", in(reg) early_trap as *const () as usize); }
}

/// 读取当前 S-mode 状态；不替调用者选择返回策略。
pub fn read_sstatus() -> usize {
    let value;
    // SAFETY: 内核在 S-mode 读取可访问 CSR。
    unsafe { core::arch::asm!("csrr {}, sstatus", out(reg) value); }
    value
}
/// # Safety
/// 调用者保证写入的 stvec 与当前陷阱/返回状态一致，且在中断关闭时操作。
pub unsafe fn write_stvec(value: usize) {
    // SAFETY: 调用者负责 CSR 值和操作时机。
    unsafe { core::arch::asm!("csrw stvec, {}", in(reg) value); }
}
/// # Safety
/// 调用者保证写入的 sepc 与当前陷阱/返回状态一致，且在中断关闭时操作。
pub unsafe fn write_sepc(value: usize) {
    // SAFETY: 调用者负责 CSR 值和操作时机。
    unsafe { core::arch::asm!("csrw sepc, {}", in(reg) value); }
}
/// # Safety
/// 调用者保证写入的 sstatus 与当前陷阱/返回状态一致，且在中断关闭时操作。
pub unsafe fn write_sstatus(value: usize) {
    // SAFETY: 调用者负责 CSR 值和操作时机。
    unsafe { core::arch::asm!("csrw sstatus, {}", in(reg) value); }
}
/// 本核执行新复制代码前同步指令缓存。
pub fn fence_i() {
    // SAFETY: S-mode 可执行，不改变内存所有权。
    unsafe { core::arch::asm!("fence.i", options(nostack)); }
}
