//! `arch::irq` — 中断使能与屏蔽 (S-mode 总开关)。
//!
//! RISC-V 中断使能是四级 (mstatus.MIE -> sstatus.SIE -> sie 分类开关 ->
//! PLIC 使能位图), 四级全开才收到中断。本模块管 `sstatus.SIE` 总开关,
//! `enable_source` 管 `sie` 分类开关 —— 分开是为暴露这个结构。

use crate::arch::csr;

/// 关中断 (清 `sstatus.SIE`)。`csrs`/`csrc` 是原子读-改-写, 不会丢位。
#[inline(always)]
pub fn disable() {
    csr::clear_sstatus(csr::SSTATUS_SIE);
}

/// 开中断 (置 `sstatus.SIE`)。
///
/// 通常不应在初始化代码里调用: 开总开关后任何已使能的分类中断都可能
/// 立刻到来, 而 `stvec` 可能还没就绪。正确顺序是装 stvec -> 配 sie/PLIC
/// -> 最后才调用本函数。
#[inline(always)]
pub fn enable() {
    csr::set_sstatus(csr::SSTATUS_SIE);
}

/// 中断总开关当前是否打开。
#[inline(always)]
pub fn is_enabled() -> bool {
    csr::read_sstatus() & csr::SSTATUS_SIE != 0
}

/// 使能某一**类**中断 (设置 `sie` 中的位); 不是"开中断", 总开关是 [`enable`]。
#[inline]
pub fn enable_source(bits: usize) {
    csr::set_sie(bits);
}

/// 使能时钟中断。
#[inline]
pub fn enable_timer() {
    csr::set_sie(csr::SIE_STIE);
}

/// 使能外部中断 (来自 PLIC)。
#[inline]
pub fn enable_external() {
    csr::set_sie(csr::SIE_SEIE);
}

/// 使能核间中断。
#[inline]
pub fn enable_software() {
    csr::set_sie(csr::SIE_SSIE);
}

/// 禁用一个类别内的所有中断。
///
/// 只应在 panic 路径上用 —— 它不区分类别, 而"某类被永久关掉"很难发现。
#[inline]
pub fn disable_all_sources() {
    // 用 `csrw sie, zero` 一次性清掉全部三类的使能位。
    // SAFETY: `sie` 在 S-mode 可读写, 写 0 屏蔽全部 S-mode 中断。
    unsafe {
        ::core::arch::asm!("csrw sie, zero", options(nomem, nostack, preserves_flags));
    }
}

/// 在关中断状态下执行 `f`, 结束恢复原状态, 返回 RAII 守卫 [`IrqGuard`]。
///
/// "恢复"而非"打开", 是为支持嵌套调用 (内层无条件开中断会破坏外层临界区)。
/// 用 RAII (`Drop`) 而非手工配对, 让"提前 return / ? 漏掉恢复"在语法上不可能。
#[inline]
pub fn save_and_disable() -> IrqGuard {
    let was_enabled = is_enabled();
    disable();
    IrqGuard { was_enabled }
}

/// [`save_and_disable`] 返回的 RAII 守卫。
#[derive(Debug)]
#[must_use = "守卫被立刻丢弃就等于没有关中断"]
pub struct IrqGuard {
    was_enabled: bool,
}

impl Drop for IrqGuard {
    #[inline]
    fn drop(&mut self) {
        if self.was_enabled {
            enable();
        }
        // 原来就是关着的 -> 保持关着。这是"恢复"而不是"打开"。
    }
}
