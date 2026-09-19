//! `arch::smp_entry` — 从核 (secondary hart) 的汇编引导 (RISC-V)。
//!
//! 从核由 SBI `HART_START` **在运行期**叫起来, 入口就是传给固件的地址,
//! 没有汇编铺垫。被唤醒时 sp / tp / satp 都未定义 (仅 `a0` = 本 hart 的
//! hartid 由固件保证), 不建栈/不设 tp/不清 satp 都会静默失败。建栈算式
//! 必须与 [`super::boot`] 的 `_entry` 完全一致。这段汇编全为 CSR 与
//! RISC-V 指令, 无 OS 语义, 所以放 arch 层。

use crate::platform::PLATFORM;

/// `KERNEL_STACK_SIZE` 的对数 (移位量); 用 `slli` 代替乘法, 不需 M 扩展。
const KERNEL_STACK_SHIFT: usize = 14;

const _: () = {
    assert!(super::boot::KERNEL_STACK_SIZE == (1 << KERNEL_STACK_SHIFT));
};

core::arch::global_asm!(
    r#"
.section .text
.align 4
.global secondary_hart_entry
secondary_hart_entry:
    # ---- 1. 校验 hartid (区间判断, 不是 hartid < ncpu) ----
    li      t0, {hart_min}
    bltu    a0, t0, .Lsec_park
    li      t0, {hart_max}
    bltu    t0, a0, .Lsec_park

    # ---- 2. 建栈: 与启动核完全相同的算式 ----
    li      t0, {hart_min}
    sub     t1, a0, t0                  # t1 = 核内序号 (从 0 开始)
    addi    t1, t1, 1                   # +1: sp 指向栈顶而不是栈底
    slli    t1, t1, {stack_shift}       # * KERNEL_STACK_SIZE
    la      sp, boot_stacks
    add     sp, sp, t1

    # ---- 3. hartid -> tp ----
    # S-mode 读不到 mhartid, 而 a0 是 caller-saved 的, 随时可能
    # 被后面的调用覆盖 —— 必须立刻搬到 tp 这个 callee-saved 的寄存器。
    mv      tp, a0

    # ---- 4. 分页关闭 (与启动核的初始状态一致) ----
    csrw    satp, zero

    # ---- 5. 关中断 ----
    # stvec 还没装好。此时来中断会跳到 stvec 的当前值 (可能是 0)。
    csrci   sstatus, 2

    # ---- 6. 进 Rust ----
    # 不传参数: Rust 侧从 tp 读 hartid, 契约尽可能小。
    call    secondary_main

.Lsec_park:
    wfi
    j       .Lsec_park
"#,
    // ---- 编译期常量 (来自 platform 模块) ----
    hart_min = const { PLATFORM.harts.min },
    hart_max = const { PLATFORM.harts.max },
    // 移位量而非乘数: 栈大小 16384 = 2^14, `slli` 足够, 不需 M 扩展。
    stack_shift = const { KERNEL_STACK_SHIFT },
);

unsafe extern "C" {
    /// 从核的汇编入口 (由上面的 `global_asm!` 定义), 交给
    /// [`crate::arch::smp::start_others`] 经 SBI 启动从核。
    #[link_name = "secondary_hart_entry"]
    pub fn SECONDARY_ENTRY() -> !;

    /// kernel 提供的从核 Rust 主函数 (`#[no_mangle] extern "C" fn secondary_main()`)。
    ///
    /// 汇编里的 `call secondary_main` 是唯一调用点, 所以 Rust 侧认为它未被
    /// 引用而报 dead_code —— 用 `#[allow(dead_code)]` 明确"是汇编在用"。
    #[allow(dead_code)]
    pub fn secondary_main() -> !;
}
