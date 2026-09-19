//! `arch::time` — 时间源。
//!
//! RISC-V 有自由运行的 `time` CSR, 频率由平台决定 (QEMU 10 MHz / JH7110
//! 4 MHz)。读时间用 `rdtime`; 设置时钟中断必须写 M-mode 的 `mtimecmp`,
//! S-mode 写不到, 所以走 SBI `set_timer` —— 本模块只暴露 `read_ticks()`
//! (读) 和 `set_next_deadline()` (走 SBI), 没有"写 mtimecmp"的入口。

use crate::arch::{csr, sbi};

/// 读 `time` CSR (单位: 平台时钟 tick)。
///
/// S-mode 可读, 不需 SBI (`csrr` 比 `ecall` 快几个数量级)。
#[inline]
pub fn read_ticks() -> u64 {
    let v: usize;
    // SAFETY: 读 `time` 无副作用; RV64 上一次 `csrr` 读完 64 位, 无撕裂。
    unsafe {
        ::core::arch::asm!("rdtime {}", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v as u64
}


/// 设置下一次时钟中断的绝对时刻, 并返回它。
///
/// 内部完成 `now + interval`, 避免调用者把**间隔**当**时刻**传
/// (那种 bug 的症状是"前几次正常, 然后突然不再抢占", 见 [`sbi::set_timer`])。
#[inline]
pub fn set_next_deadline(interval: u64) -> u64 {
    let now = read_ticks();
    let deadline = now.wrapping_add(interval);
    sbi::set_timer(deadline);
    deadline
}

/// 时钟频率 (Hz) 的估算值, 仅供启动横幅把 tick 换算成秒。
///
/// 不在 platform 结构体里 —— 内核行为不需要它, 只在打印时用
/// (两个平台都按 0.1 秒配置, 所以频率 = 间隔 × 10)。
pub fn ticks_per_second_hint() -> u64 {
    (crate::platform::PLATFORM.timer_interval as u64) * 10
}

/// 忙等 `ticks` 个时钟 tick。
///
/// 中断关闭时会让 CPU 空转, ticks 过大可能变成死循环; 只在启动早期
/// (时钟中断未配置但 `time` 可用) 用于等待 UART FIFO、SD 卡上电等。
///
/// # Safety
/// 只在中断关闭状态下有意义 (否则会被抢占); 调用者须保证 ticks
/// 不会导致不可接受的长时间挂起。
pub unsafe fn busy_wait_ticks(ticks: u64) {
    let start = read_ticks();
    while read_ticks().wrapping_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

/// 关闭当前 hart 的中断并永久等待。
///
/// 这是"该 hart 不应运行"时的最终归宿 (见 `boot` 对非法 hartid 的处理)。
pub fn park_current_hart() -> ! {
    csr::clear_sstatus(csr::SSTATUS_SIE);
    loop {
        // SAFETY: `wfi` 在中断关闭时一直等到下一个中断, 然后因 SIE=0
        // 立即继续执行 wfi —— 正是"永久停放"。
        unsafe {
            ::core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
    }
}
