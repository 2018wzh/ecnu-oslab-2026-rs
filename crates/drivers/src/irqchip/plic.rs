//! SiFive PLIC 驱动。QEMU virt 和 VisionFive2 都集成 PLIC, 差别只在
//! 基地址 (由 platform 层提供), 所以一份源码服务两个平台。
//!
//! claim/complete 必须配对: 本驱动用 RAII 守卫 [`Claim`], 在 `Drop` 里
//! 自动 complete, 防止"忘写回导致中断只来一次"。
//!
//! per-hart 寄存器用的是 **hartid** 而非内核 cpuid (硬件只认 hartid);
//! QEMU 上两者相同, VF2 上差 1, 差异限制在本文件内部。

use oslab_hal::platform::Platform;

use crate::mmio::Mmio;

// ---------------------------------------------------------------------------
// 寄存器偏移 (相对 PLIC 基地址) —— 这些是 PLIC 的**协议**, 属于驱动。
// ---------------------------------------------------------------------------

/// 中断源优先级数组的起始偏移。
const PRIORITY_BASE: usize = 0x0000_0000;
/// M-mode 使能位图的起始偏移。
#[allow(dead_code)]
const ENABLE_M_BASE: usize = 0x0000_2000;
/// S-mode 使能位图的起始偏移。
const ENABLE_S_BASE: usize = 0x0000_2080;
/// 每个 hart 在使能位图里占用的字节数。
const ENABLE_STRIDE: usize = 0x100;
/// M-mode hart 优先级阈值的起始偏移。
const CONTEXT_M_BASE: usize = 0x0020_0000;
/// S-mode hart 优先级阈值的起始偏移。
const CONTEXT_S_BASE: usize = 0x0020_1000;
/// 每个 hart 在 context 区域里占用的字节数 (0x2000, 注意与使能位图步长不同)。
const CONTEXT_STRIDE: usize = 0x2000;

/// 一个中断源的优先级。
///
/// 0 表示"不产生中断"; 本内核把需要的中断源都设为 1 (相同优先级, PLIC 轮转)。
const PRIORITY_DEFAULT: u32 = 1;

/// 优先级阈值。PLIC 阈值语义是"严格大于": 阈值为 0 接受所有优先级 > 0
/// 的中断; 设为 1 会屏蔽掉所有优先级为 1 的中断。
const THRESHOLD_ACCEPT_ALL: u32 = 0;

/// PLIC 操作可能的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlicError {
    /// 请求使能的中断号超出 PLIC 的支持范围。
    IrqOutOfRange,
    /// 中断号 0 是保留值, 表示"没有中断", 不能被使能。
    IrqZeroReserved,
}

/// 中断号的最大值。
///
/// PLIC 规范允许最多 1024 个中断源, 用 1024 作上界只验证"不越界写优先级数组"。
pub const MAX_IRQ: u32 = 1024;

/// SiFive PLIC 驱动实例。
///
/// 只有两个 `usize` 和一个 u32, 刻意 `Copy`, 便于按值传递 (per-cpu 场景)。
#[derive(Debug, Clone, Copy)]
pub struct Plic {
    regs: Mmio,
    /// PLIC 窗口大小, 用于页表映射时算页数。
    size: usize,
}

impl Plic {
    /// 从平台描述构造驱动实例 (不做任何硬件访问)。
    ///
    /// # Safety
    /// `plat.plic_base` 必须指向真实 PLIC 且已映射。
    pub const unsafe fn new(plat: &Platform) -> Self {
        Self {
            // SAFETY: 由调用者保证。
            regs: unsafe { Mmio::new(plat.plic_base) },
            size: plat.plic_size,
        }
    }

    /// 基地址。
    pub const fn base(&self) -> usize {
        self.regs.base()
    }

    /// 窗口大小。
    pub const fn size(&self) -> usize {
        self.size
    }

    // -----------------------------------------------------------------------
    // 全局初始化 (只需在一个 hart 上执行一次)
    // -----------------------------------------------------------------------

    /// 设置一个中断源的优先级。
    ///
    /// 使能一个源前必须先设优先级: 优先级为 0 的源即使被使能也不产生中断。
    pub fn set_priority(&self, irq: u32, priority: u32) -> Result<(), PlicError> {
        if irq == 0 {
            // 中断号 0 是保留值 ("没有中断"), 它的优先级寄存器不存在。
            return Err(PlicError::IrqZeroReserved);
        }
        if irq >= MAX_IRQ {
            return Err(PlicError::IrqOutOfRange);
        }
        // SAFETY: irq < MAX_IRQ 保证了偏移落在优先级数组内。
        unsafe {
            self.regs
                .write_u32(PRIORITY_BASE + (irq as usize) * 4, priority);
        }
        Ok(())
    }

    /// 读一个中断源的优先级 (用于验证初始化是否生效)。
    pub fn priority(&self, irq: u32) -> u32 {
        if irq == 0 || irq >= MAX_IRQ {
            return 0;
        }
        // SAFETY: 同上。
        unsafe { self.regs.read_u32(PRIORITY_BASE + (irq as usize) * 4) }
    }

    /// 为某个 hart 使能一个中断源。
    ///
    /// 参数是 `hartid` 而非 `cpuid`: PLIC 的 per-hart 寄存器由硬件直接
    /// 寻址, 硬件只认 hartid (QEMU 上两者相同, VF2 上 `hartid == cpuid + 1`)。
    /// 调用者可用 `platform.harts.to_hartid(cpuid)` 换算。
    pub fn enable(&self, hartid: usize, irq: u32) -> Result<(), PlicError> {
        if irq == 0 {
            return Err(PlicError::IrqZeroReserved);
        }
        if irq >= MAX_IRQ {
            return Err(PlicError::IrqOutOfRange);
        }
        // S-mode 使能位图是按 32 位字组织的: 第 irq 号源在第
        // (irq / 32) 个字里, 占第 (irq % 32) 位。
        let word = (irq / 32) as usize;
        let bit = irq % 32;
        let addr = ENABLE_S_BASE + hartid * ENABLE_STRIDE + word * 4;
        // SAFETY: irq < MAX_IRQ 保证了 word < 32, 而 PLIC 为每个
        // hart 在使能位图区预留了 0x100 字节 = 64 个字, 足够。
        unsafe {
            self.regs.set_bits_u32(addr, 1 << bit);
        }
        Ok(())
    }

    /// 为一个 hart 使能一组中断源。
    ///
    /// 传入中断号切片而非位图, 把 `1 << 32` 这类危险移位收进驱动内部。
    pub fn enable_many(&self, hartid: usize, irqs: &[u32]) -> Result<(), PlicError> {
        for &irq in irqs {
            self.enable(hartid, irq)?;
        }
        Ok(())
    }

    /// 设置某个 hart 的优先级阈值。
    ///
    /// 见 [`THRESHOLD_ACCEPT_ALL`] 关于"严格大于"语义的说明。
    pub fn set_threshold(&self, hartid: usize, threshold: u32) {
        let addr = CONTEXT_S_BASE + hartid * CONTEXT_STRIDE;
        // SAFETY: context 区为每个 hart 预留了 0x2000 字节,
        // 阈值在偏移 0 处。
        unsafe {
            self.regs.write_u32(addr, threshold);
        }
    }

    /// 读某个 hart 的优先级阈值。
    pub fn threshold(&self, hartid: usize) -> u32 {
        let addr = CONTEXT_S_BASE + hartid * CONTEXT_STRIDE;
        // SAFETY: 同上。
        unsafe { self.regs.read_u32(addr) }
    }

    /// 完成一个 hart 的全部初始化: 设优先级、设阈值、使能给定的中断源。
    ///
    /// 启动流程调用这个入口, 把顺序错误时"中断永不到来"的难查问题
    /// 封在函数里。优先级重复设置是幂等的, 所以全局部分不区分启动核。
    pub fn init_hart(&self, hartid: usize, irqs: &[u32]) -> Result<(), PlicError> { unimplemented!() }

    // -----------------------------------------------------------------------
    // 运行期: claim / complete
    // -----------------------------------------------------------------------

    /// 认领一个待处理的中断。
    ///
    /// 返回 `Some(Claim)` 时中断已被认领, 守卫被丢弃时自动 complete。
    /// `None` 表示没有待处理中断 (多核下是正常情况, 可能被别的核先认领了)。
    /// 返回守卫而非中断号, 让"认领/完成"配对成为语法保证。
    pub fn claim(&self, hartid: usize) -> Option<Claim<'_>> { unimplemented!() }
}

/// 已认领的中断。被丢弃时自动 complete。
///
/// 不实现 `Copy`/`Clone`: "一个中断被完成两次"是真实错误, 禁止复制
/// 让这个错误无法表达。
#[derive(Debug)]
#[must_use = "认领了中断却不处理, 等于把它丢掉后立刻 complete"]
pub struct Claim<'a> {
    plic: &'a Plic,
    hartid: usize,
    irq: u32,
    completed: bool,
}

impl<'a> Claim<'a> {
    /// 认领到的中断号。
    pub fn irq(&self) -> u32 {
        self.irq
    }

    /// 显式完成 (一般不需要, 丢弃守卫时会自动完成)。
    /// 供那些"需要先完成再继续做长事情"的驱动使用。
    pub fn complete(mut self) {
        self.do_complete();
    }

    fn do_complete(&mut self) {
        if self.completed {
            return;
        }
        let addr = CONTEXT_S_BASE + self.hartid * CONTEXT_STRIDE + 4;
        // SAFETY: 同 `claim`。写回同一个中断号是 PLIC 的约定。
        unsafe {
            self.plic.regs.write_u32(addr, self.irq);
        }
        self.completed = true;
    }
}

impl<'a> Drop for Claim<'a> {
    fn drop(&mut self) {
        // 这就是"忘记 complete"这个 bug 的解药。
        // `do_complete` 用 `completed` 标志保证幂等, 不会 double complete。
        self.do_complete();
    }
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // 使能位图的步长必须是 4 的倍数 (它是按 32 位字组织的)。
    assert!(ENABLE_STRIDE % 4 == 0);
    // S-mode 使能位图区 (0x2080) 加每个 hart 0x100 字节, 不能越入 context 区,
    // 否则会写坏另一个 hart 的阈值寄存器。
    assert!(ENABLE_S_BASE + 64 * ENABLE_STRIDE <= CONTEXT_M_BASE);
    // context 区必须能容纳所有 hart 的阈值寄存器 (最大 hart 数 64)。
    assert!(CONTEXT_M_BASE + 64 * CONTEXT_STRIDE <= CONTEXT_S_BASE + 64 * CONTEXT_STRIDE);
};
