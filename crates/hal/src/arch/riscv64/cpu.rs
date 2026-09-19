//! `arch::cpu` — 处理器身份与拓扑。
//!
//! S-mode 读不到 `mhartid` (那是 M-mode CSR), 所以启动时把固件传来的
//! hartid 存进 `tp` 寄存器, 之后永远从 `tp` 取。用 `tp` 零成本, 但
//! 上下文切换时必须保存/恢复它。

use crate::platform::{self, Platform};

/// 当前机器 (编译期求值成常量, 无运行期开销)。
#[inline(always)]
pub const fn platform() -> &'static Platform {
    &platform::PLATFORM
}

/// 读 `tp` 寄存器, 拿到**硬件 hart id** (不是内核 cpuid, 后者见 [`cpu_id`])。
#[inline(always)]
pub fn hartid() -> usize {
    let v: usize;
    // SAFETY: `tp` 是通用寄存器, 读它没有副作用; 启动代码已把它设为 hartid。
    unsafe {
        ::core::arch::asm!("mv {}, tp", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// 写 `tp`。
///
/// # Safety
/// 只有启动代码和上下文切换代码可以调用。随意改写会让 [`hartid`] 错误,
/// 破坏所有 per-cpu 数据结构 (栈、idle 进程、PLIC 使能位图) 的索引。
#[inline(always)]
pub unsafe fn set_hartid(hartid: usize) {
    // SAFETY: 由调用者保证。
    unsafe {
        ::core::arch::asm!("mv tp, {}", in(reg) hartid, options(nomem, nostack, preserves_flags));
    }
}

/// 当前 CPU 的**内核编号** (0 起, 连续), 用于索引 per-cpu 数组。
///
/// 返回 `None` 表示当前 hart 不在平台允许范围内 —— 启动代码应停掉它。
/// 返回 `Option` 而非 clamp, 是让调用者必须面对"这个 hart 可能不该运行"。
#[inline]
pub fn cpu_id() -> Option<usize> {
    platform().harts.to_cpu_id(hartid())
}

/// 当前 hart 是否被允许运行内核。
#[inline]
pub fn hart_is_valid() -> bool {
    platform().harts.contains(hartid())
}

/// 冷启动核的 hartid —— **运行期事实**, 由启动汇编的抽签记录。
///
/// 不能直接读 `platform().boot_hart`: 那是固件启动核的**预期值**
/// (OpenSBI 的冷启动核是抽签的), 与启动汇编用原子交换记下的
/// 实际启动核可能不一致。
#[inline]
pub fn cold_boot_hart() -> usize {
    // 抽签锁里存的是赢家的 `hartid + 1`, 区分"没认领"(0) 与"启动核是 hart 0"(1)。
    // 只在 [`boot_claim_ok`] 为 true 时有意义。
    boot_claim_raw().saturating_sub(1)
}

/// 启动抽签是否真的发生过 (同时意味着 `.bss` 已被清)。
///
/// 值为 false 时内核处于未定义状态, 启动早期必须停下来并说清楚。
#[inline]
pub fn boot_claim_ok() -> bool {
    boot_claim_raw() != 0
}

#[inline]
fn boot_claim_raw() -> usize {
    // 值由启动汇编写入, Rust 侧从没写过, 普通读可能被优化成常量, 改用 volatile。
    //
    // SAFETY: 启动汇编在跳进 Rust 前写入, 之后不再改动。
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(COLD_BOOT_HART)) as usize }
}

unsafe extern "C" {
    /// 启动汇编记录的冷启动核身份 `hartid + 1` (在 `.data` 里)。0 = 未认领。
    #[link_name = "__cold_boot_hart"]
    static COLD_BOOT_HART: u64;
}

/// 是否是**冷启动核** (固件实际交给内核的那个 hart); 负责"全局只需一次"的初始化。
#[inline]
pub fn is_boot_hart() -> bool {
    hartid() == cold_boot_hart()
}

// ===========================================================================
// 编译期断言: 平台拓扑自洽
// ===========================================================================
// 这几条对所有平台都成立, 所以放在 arch 层。
const _: () = {
    // 至少要有一个 hart。
    assert!(cpu_topology_ncpu() > 0);
    // 区间长度必须等于 ncpu ("4 核只起来 3 个"就是违反这条的结果)。
    assert!(cpu_topology_count() == cpu_topology_ncpu());
    // 启动 hart 必须在合法区间内。
    assert!(cpu_topology_min() <= cpu_topology_boot() && cpu_topology_boot() <= cpu_topology_max());
};

/// 编译期读取 [`Platform::ncpu`]。
pub const fn cpu_topology_ncpu() -> usize {
    platform::PLATFORM.ncpu
}
/// 编译期读取 hart 区间起点。
pub const fn cpu_topology_min() -> usize {
    platform::PLATFORM.harts.min
}
/// 编译期读取 hart 区间终点。
pub const fn cpu_topology_max() -> usize {
    platform::PLATFORM.harts.max
}
/// 编译期读取启动 hart。
pub const fn cpu_topology_boot() -> usize {
    platform::PLATFORM.boot_hart
}
/// 编译期读取区间长度。
pub const fn cpu_topology_count() -> usize {
    platform::PLATFORM.harts.count()
}

// ===========================================================================
// 核间中断 (IPI)
// ===========================================================================
// 一个 CPU 让另一个 CPU 立刻做事 (刷新 TLB、重新调度), 通过
// S-mode 软件中断 (`sie.SSIE`) 与 SBI 的 IPI 扩展触发。

/// 向一组 hart 发送核间中断, `hart_mask` 的每一位对应一个 hart id。
///
/// SBI v0.2 IPI 用位图编码: 第 i 位表示 "hart i"。
pub fn send_ipi(hart_mask: usize) {
    // 转发到 sbi 的公开函数, 保持"只有一处执行 ecall"。
    crate::arch::sbi::send_ipi_raw(hart_mask);
}

/// 轮询等待, 用于早期启动时"给其他 hart 一点时间" (启动早期没有可靠定时器)。
#[inline]
pub fn spin_delay(cycles: usize) {
    for _ in 0..cycles {
        // `spin_loop` 而非空循环: 空循环会被 LLVM 优化掉, 且 spin_loop 可省电。
        core::hint::spin_loop();
    }
}
