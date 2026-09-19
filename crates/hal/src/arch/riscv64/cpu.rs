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
