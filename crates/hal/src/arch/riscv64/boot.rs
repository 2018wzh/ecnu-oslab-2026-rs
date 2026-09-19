//! `arch::boot` — 内核启动汇编入口 (`_entry`) 及汇编用到的常量。
//!
//! 进入时的 RISC-V S-mode 启动 ABI (与 Linux 相同): `a0` = hartid,
//! `satp` = 0 (物理地址运行), `sp` 未定义 (内核自建栈)。汇编通过
//! `global_asm!` 的 `const` 操作数直接读 platform 常量, 不抄第二份定义。

use crate::platform::PLATFORM;

/// 每个 hart 的内核栈大小 (字节), 16 KiB。
///
/// 足够容纳"块缓冲 + trap 处理 + 打印格式化"的调用链, 又不过多占用
/// (4 个 hart 共 64 KiB)。栈上的局部大对象会写穿这个大小, 已修复。
pub const KERNEL_STACK_SIZE: usize = 16 * 1024;

/// 内核栈槽位总数 = `ncpu + 1`。
///
/// 多留一页: hartid 换算出错导致越界一页时不立刻踩坏别的数据,
/// 而是被 canary 检查发现。
pub const KERNEL_STACK_SLOTS: usize = PLATFORM.ncpu + 1;

/// 栈 canary 的魔数。见 [`check_stack_canary`]。
pub const STACK_CANARY: usize = 0x5A5A_5A5A_5A5A_5A5A;

// ===========================================================================
// 入口汇编
// ===========================================================================
// `global_asm!` 是模块级宏; 下面所有 `{...}` 都是编译期常量。
core::arch::global_asm!(
    r#"
# 本文件只有一处原子指令 (cold boot 抽签用的 amoswap.w)。新版 LLVM 把
# RISC-V 的 A 扩展细分成 Zaamo/Zalrsc 后, 内联汇编解析器不再默认打开它,
# 必须显式声明。
.option arch, +a

.section .text.entry
.global _entry
_entry:
    # ---- 第 1 步: 校验 hartid 是否属于内核 ----
    # 平台编号起点不同 (QEMU: 0..1; VF2: 1..4), 所以用区间判断而非
    # `hartid < ncpu`, 否则 VF2 上合法 hart 4 会被误判为非法而不启动。
    li      t0, {hart_min}
    bltu    a0, t0, .Lpark              # hartid < min -> 非法, 停掉它
    li      t0, {hart_max}
    bltu    t0, a0, .Lpark              # hartid > max -> 非法, 停掉它

    # ---- 第 2 步: 建立本 hart 的内核栈 ----
    # boot_stacks: [slot 0][slot 1][slot 2]... (链接脚本分配 ncpu+1 个槽位)
    # sp = boot_stacks + (hartid - min + 1) * KERNEL_STACK_SIZE
    # 用 (hartid - min) 而非 hartid: VF2 的 min=1, 直接索引会浪费 slot 0 且越界。
    # +1 让 sp 指向栈顶 (高地址) 而不是栈底。
    li      t0, {hart_min}
    sub     t1, a0, t0                  # t1 = 核内序号 (从 0 开始)
    addi    t1, t1, 1                   # +1, 让 sp 指向栈顶而不是栈底
    # 用左移代替乘法: 栈大小 4096 = 2^12 (下面有编译期断言守着幂次)。
    slli    t1, t1, 12
    la      sp, boot_stacks
    add     sp, sp, t1
    # boot_stacks 由链接脚本提供; 即使 lab-1 删掉从核汇编也要保留对它的引用,
    # 否则链接器会当作未使用符号连同一起丢弃 (报 "undefined symbol boot_stacks")。

    # ---- 第 3 步: 把 hartid 存进 tp ----
    # S-mode 读不到 mhartid, 这是唯一一次能拿到它的机会, 之后都从 tp 读。
    # 用 tp 而非全局变量: 启动早期多核访问全局变量会竞争, tp 每 hart 私有。
    mv      tp, a0

    # ---- 第 4 步: 保住 a0 ----
    # (a0 已在 tp 里, Rust 侧从 tp 读 hartid, 无需额外保存。)

    # ---- 第 5 步: 认领"冷启动核"并清 .bss (一次原子交换做两件事) ----
    # 加载器 (QEMU ELF 或 U-Boot objcopy 裸二进制) 都不保证 .bss 清零。
    # 必须恰好一个 hart 清: 每个 hart 都清会把先启动核写好的数据抹掉。
    # 谁清由 amoswap 抽签决定, 不查平台常量 boot_hart —— 固件可能不照办
    # (OpenSBI 冷启动核是抽签出的), 那只是个预期值。
    # 认领值 = hartid + 1, 随交换原子地落进锁里, 赢家身份同步可读。
    # 锁放 .data 而非 .bss: 它要在这段清零之前就可读, .bss 在裸二进制路径上是垃圾。
    la      t0, __cold_boot_lottery
    li      t1, 1
    amoswap.w t2, t1, (t0)              # t2 = 交换前的值 (32 位, **带符号扩展**)
    # amoswap.w 结果是符号扩展: 哨兵最高位为 1 时 t2 = 0xffffffff_b007b007,
    # 必须零扩展后才能与哨兵/0 比较。
    slli    t2, t2, 32
    srli    t2, t2, 32
    li      t3, {lottery_free}
    beq     t2, t3, .Lwinner            # 还是哨兵 -> 我是第一个
    beqz    t2, .Lwinner                # 是 0 -> 哨兵没被加载, 也当第一个
    j       .Lwait_claim                # 别人先认领了, 去等它把身份写下来

.Lwinner:
    # 先把身份 (hartid + 1) 记在 .data 里再清 .bss: 0 专门表示"还没记下来",
    # 与"启动核恰好是 hart 0"区分开。放 .data 而非 .bss: 后者会被下面清零抹掉。
    la      t0, __cold_boot_hart
    addi    t1, a0, 1
    sd      t1, 0(t0)

    la      t0, __bss_start
    la      t1, __bss_end
.Lbss_loop:
    bgeu    t0, t1, .Lskip_bss
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       .Lbss_loop

.Lwait_claim:
    # 输家等赢家写下身份再走: 窗口只有两条指令, 很短。自旋上限给"哨兵是
    # 垃圾值、谁都没赢"的病态情况留出路, 由 Rust 侧去报明确错误。
    la      t0, __cold_boot_hart
    li      t1, {claim_spin}
.Lwait_loop:
    ld      t2, 0(t0)
    bnez    t2, .Lskip_bss
    addi    t1, t1, -1
    bnez    t1, .Lwait_loop

.Lskip_bss:
    # ---- 第 6 步: 给内核栈打 canary ----
    # 在栈最低 (最先被溢出踩到) 的地址写魔数; 被改说明栈溢出 (见 check_stack_canary)。
    la      t0, stack_canaries
    li      t1, {hart_min}
    sub     t2, a0, t1
    slli    t2, t2, 3                   # 每个 canary 8 字节
    add     t0, t0, t2
    li      t1, {canary}
    sd      t1, 0(t0)

    # ---- 第 7 步: satp = 0 (分页关闭) ----
    # 固件应当已清零, 但花一条指令排除"残留非零 satp"导致的不可预测。
    csrw    satp, zero

    # ---- 第 8 步: 关中断 ----
    # stvec 还没装好, 此时来中断会跳到 stvec 当前值 (可能为 0) 执行垃圾。
    # csrci 立即数 2 = 1 << 1 = sstatus.SIE。
    csrci   sstatus, 2

    # ---- 第 9 步: 进 Rust ----
    # 不传参数: Rust 侧从 tp 读 hartid, 尽量缩小入口契约。
    call    kernel_entry

    # kernel_entry 不该返回; 返回说明出了严重问题。
.Lpark:
    wfi
    j       .Lpark

    # ---- 冷启动核抽签用的两个符号 ----
    # 哨兵放 .data: 运行前就需确定值, 而 .bss 在裸二进制路径上是垃圾。
    .section .data
    .align 3
__cold_boot_lottery:
    .quad {lottery_free}

    # 冷启动核身份 (hartid + 1), 0 = 还没认领。须放 .data (要在清 .bss 前写好)。
    .align 3
.global __cold_boot_hart
__cold_boot_hart:
    .quad 0
"#,
    // ---- 编译期常量 (global_asm! 的模板参数必须放在模板之后) ----
    hart_min = const { PLATFORM.harts.min },
    hart_max = const { PLATFORM.harts.max },
    canary = const { STACK_CANARY },
    lottery_free = const { LOTTERY_FREE },
    claim_spin = const { CLAIM_SPIN },
);

/// 抽签哨兵的初值 (非零, 放 `.data` 而非 `.bss`)。
///
/// 0 的变量会被放进 `.bss`, 而裸二进制加载路径不搬运 `.bss`,
/// 哨兵会变成垃圾, 抽签结果不可预测。
pub const LOTTERY_FREE: u64 = 0xB007_B007;

/// 输家等待"赢家写下身份"的自旋上限。
///
/// 正常最多转一两圈; 上限给"哨兵是垃圾值、谁都没赢"的病态情况留出路,
/// 宁可由 Rust 侧报错, 也不在汇编里静默死等。
pub const CLAIM_SPIN: usize = 1 << 20;

// ===========================================================================
// 编译期断言: 启动汇编的假设
// ===========================================================================
const _: () = {
    // 汇编用 `slli` 代替乘法, 要求栈大小恰好是 2 的幂, 移位量由
    // `trailing_zeros()` 守着 —— 改 `KERNEL_STACK_SIZE` 必须同步改移位量。
    assert!(KERNEL_STACK_SIZE.is_power_of_two());
    assert!(KERNEL_STACK_SIZE.trailing_zeros() == 14);

    // 栈槽位数必须至少覆盖所有 hart ("ncpu + 1" 多出的一页是越界余量)。
    assert!(KERNEL_STACK_SLOTS > PLATFORM.ncpu);
};

// ===========================================================================
// Rust 侧的入口
// ===========================================================================

unsafe extern "C" {
    /// 由链接脚本提供: `.bss` 段的起点。用 `extern "C"` 而非 `static`
    /// 声明, 因为链接脚本符号无类型, `static` 会给错误的大小/对齐假设。
    pub static __bss_start: u8;
    /// `.bss` 段的终点。
    pub static __bss_end: u8;
    /// 内核栈数组的起点。
    pub static boot_stacks: u8;
    /// 栈 canary 数组的起点。
    pub static stack_canaries: u8;
    /// 内核镜像的终点 (物理页分配器的起点)。
    pub static _kernel_end: u8;
    /// 内核入口 (由本模块的 `global_asm!` 定义)。
    pub fn _entry();
}

// Rust 侧的内核入口, 由汇编调用, 由 `kernel` crate 实现。
// 用 `extern "C"` 保证导出名不被 name-mangling (`call kernel_entry`)。
unsafe extern "C" {
    /// 内核主入口 (见 crates/kernel/src/main.rs)。
    pub fn kernel_entry() -> !;
}

/// 检查某个 CPU 的栈 canary 是否完好 (返回 `true` 表示栈没溢出)。
///
/// 在每次时钟中断里检查一次, 代价是一次内存读, 让栈溢出在发生瞬间
/// 被报告, 而不是几百万条指令之后。
pub fn check_stack_canary(cpuid: usize) -> bool {
    // SAFETY: `stack_canaries` 由链接脚本分配了 KERNEL_STACK_SLOTS 个
    // usize; cpuid 来自 `cpu_id()` 落在 0..ncpu 内, 索引一定在界内。
    unsafe {
        let base = &stack_canaries as *const u8 as *const usize;
        core::ptr::read_volatile(base.add(cpuid)) == STACK_CANARY
    }
}

// ===========================================================================
// 链接脚本符号的引用 (以及为什么 lab-1 不需要从核也能链接)
// ===========================================================================
// `boot_stacks` 与 `stack_canaries` 由链接脚本定义。除了两处汇编引用外,
// 这里的 `extern` 声明与 Rust 侧的`check_stack_canary` 也引用它们 ——
// 即使删掉从核汇编, 这两个符号也不会被链接器当作"未使用"而丢弃
// (汇编里的引用是弱证据, Rust 代码里的引用是强的)。
