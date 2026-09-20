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
