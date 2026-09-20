//! CPU 身份与嵌套中断开关。
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use crate::platform::{HART_FIRST, NCPU};
use super::{csr, sbi};
static DEPTH: [AtomicUsize; NCPU] = [const { AtomicUsize::new(0) }; NCPU];
static ENABLED: [AtomicBool; NCPU] = [const { AtomicBool::new(false) }; NCPU];
pub fn hart_id() -> usize {
    let id;
    // SAFETY: 汇编入口将当前 hartid 写入 tp。
    unsafe { core::arch::asm!("mv {}, tp", out(reg) id); }
    id
}
pub fn cpu_id() -> usize { hart_id() - HART_FIRST }
pub fn is_boot_cpu() -> bool {
    unsafe extern "C" { static boot_hart: usize; }
    // SAFETY: boot_hart 在启动其他 CPU 前写入，此后不变。
    hart_id() == unsafe { boot_hart }
}
pub fn start_cpu(cpu: usize) -> Result<(), sbi::SbiError> {
    unsafe extern "C" { fn secondary_entry(); }
    if cpu >= NCPU || cpu == cpu_id() { return Err(sbi::SbiError(-3)); }
    sbi::hart_start(cpu + HART_FIRST, secondary_entry as *const () as usize)
}
pub fn park() -> ! {
    loop {
        // SAFETY: wfi 不访问内存，不要求中断一定唤醒。
        unsafe { core::arch::asm!("wfi"); }
    }
}
// 开关中断的基本逻辑：
// 1. 多处可能开关中断，因此不是“开/关”的二元状态，而是“关 关 关 开 开 开”的 stack。
// 2. 第一次关中断时记录初始状态 X。
// 3. 每次关中断，stack 中的元素加 1。
// 4. 每次恢复，stack 中的元素减 1。
// 5. stack 清空时，将中断状态恢复为初始的 X。
// 每核仅访问自己的状态，不跨核转交；push_off/pop_off 必须配对。
pub fn push_off() {
    let old = csr::irq_enabled();
    csr::irq_disable();
    let cpu = cpu_id();
    if DEPTH[cpu].load(Relaxed) == 0 { ENABLED[cpu].store(old, Relaxed); }
    DEPTH[cpu].fetch_add(1, Relaxed);
}
pub fn pop_off() {
    let cpu = cpu_id();
    assert!(!csr::irq_enabled() && DEPTH[cpu].load(Relaxed) > 0);
    if DEPTH[cpu].fetch_sub(1, Relaxed) == 1 && ENABLED[cpu].load(Relaxed) { csr::irq_enable(); }
}

/// 教师外围：仅关中断时读取本核嵌套深度，用于切换边界检查。
pub fn interrupt_depth() -> usize {
    assert!(!csr::irq_enabled());
    DEPTH[cpu_id()].load(Relaxed)
}

/// 教师外围：唯一锁释放后应恢复的中断策略；仅限关闭中断、depth=1。
pub fn resume_interrupts() -> bool {
    assert_eq!(interrupt_depth(), 1);
    ENABLED[cpu_id()].load(Relaxed)
}
/// 恢复逻辑调用者的中断策略，绝不复制其他 CPU 的 depth 或锁 owner。
pub fn set_resume_interrupts(enabled: bool) {
    assert_eq!(interrupt_depth(), 1);
    ENABLED[cpu_id()].store(enabled, Relaxed);
}

/// DMA/MMIO 访问排序；这不是缓存刷新。
pub fn dma_fence() {
    // SAFETY: fence 只排序访问，不修改寄存器上下文或控制流。
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)); }
}
