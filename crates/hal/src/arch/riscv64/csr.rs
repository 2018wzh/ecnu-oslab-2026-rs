//! RISC-V 控制状态寄存器 (CSR)。这是 ISA 特有机制 (AArch64 是系统寄存器),
//! 所以定义必须待在 arch 层, 上层只能通过语义化函数访问 (如 `irq::disable`),
//! 不直接读写 CSR。
//!
//! 读写宏都是私有宏 (`macro_rules!` 默认不导出), 外部只能用有明确语义的函数。

// ===========================================================================
// 私有的 CSR 读写原语
// ===========================================================================
// 这些宏只在本模块内可见, "谁能读写 CSR"由模块边界精确控制。

macro_rules! csrr {
    ($reg:literal) => {{
        let v: usize;
        // SAFETY: 读取 CSR 没有副作用 (除了清掉某些 CSR 的"已读"位,
        // 例如 `sip` —— 但我们不在这里读那些)。`nomem` 是错的:
        // CSR 的读写会与内存访问重排, 比如先写 satp 再读它。
        unsafe {
            ::core::arch::asm!(concat!("csrr {}, ", $reg), out(reg) v, options(nomem, nostack));
        }
        v
    }};
}

macro_rules! csrw {
    ($reg:literal, $val:expr) => {{
        let v: usize = $val;
        // SAFETY: 由调用者保证目标 CSR 在当前特权级可写。
        // 本模块内所有调用点都在 S-mode 下运行, 且只写 S-mode CSR。
        unsafe {
            ::core::arch::asm!(concat!("csrw ", $reg, ", {}"), in(reg) v, options(nomem, nostack));
        }
    }};
}

macro_rules! csrs {
    ($reg:literal, $bits:expr) => {{
        let v: usize = $bits;
        // SAFETY: 同 csrw。
        unsafe {
            ::core::arch::asm!(concat!("csrs ", $reg, ", {}"), in(reg) v, options(nomem, nostack));
        }
    }};
}

macro_rules! csrc {
    ($reg:literal, $bits:expr) => {{
        let v: usize = $bits;
        // SAFETY: 同 csrw。
        unsafe {
            ::core::arch::asm!(concat!("csrc ", $reg, ", {}"), in(reg) v, options(nomem, nostack));
        }
    }};
}

#[allow(unused_imports)]
pub(crate) use {csrc, csrr, csrs, csrw};

// ===========================================================================
// sstatus
// ===========================================================================
// 位号由 RISC-V 特权级规范规定, 与具体芯片无关, 所以属于 arch 层。

/// S-mode 中断使能。
pub const SSTATUS_SIE: usize = 1 << 1;
/// 进入 trap 前的中断使能状态。`sret` 之后 SIE 会被设成它。
pub const SSTATUS_SPIE: usize = 1 << 5;
/// 进入 trap 前的特权级: 1 = S-mode, 0 = U-mode。
pub const SSTATUS_SPP: usize = 1 << 8;
/// 允许 S-mode 读写带 U 位的页面 (内核 `copyin`/`copyout` 时需要)。
pub const SSTATUS_SUM: usize = 1 << 18;
/// S-mode 是否可读浮点状态 (一般保持 1)。
pub const SSTATUS_FS: usize = 3 << 13;

/// 读 `sstatus`。
#[inline]
pub fn read_sstatus() -> usize {
    csrr!("sstatus")
}

/// 写 `sstatus`。
///
/// # Safety
/// 调用者必须理解自己改的位, 特别是不要无故改动 SPP (它决定下次
/// `sret` 回到哪个特权级)。
#[inline]
pub unsafe fn write_sstatus(v: usize) {
    csrw!("sstatus", v);
}

/// 置位 `sstatus` 的某些位。
#[inline]
pub fn set_sstatus(bits: usize) {
    csrs!("sstatus", bits);
}

/// 清掉 `sstatus` 的某些位。
#[inline]
pub fn clear_sstatus(bits: usize) {
    csrc!("sstatus", bits);
}

// ===========================================================================
// sie / sip
// ===========================================================================
// 中断使能是四级结构: mstatus.MIE (M-mode 总开关) -> sstatus.SIE
// (S-mode 总开关, `irq::enable/disable` 管这里) -> sie.SSIE/STIE/SEIE
// (分类开关, `irq::enable_source` 管这里) -> PLIC 使能位图 (单个设备)。
// 四级全开才真正收到中断, 缺任何一级都是"中断不来", 但原因不同。

/// 软件中断使能位。
pub const SIE_SSIE: usize = 1 << 1;
/// 时钟中断使能位。
pub const SIE_STIE: usize = 1 << 5;
/// 外部中断使能位。
pub const SIE_SEIE: usize = 1 << 9;

/// 读 `sie`。
#[inline]
pub fn read_sie() -> usize {
    csrr!("sie")
}

/// 置位 `sie` 中的位。
///
/// 注意这不是"开中断": 只打开某一**类**中断, 总开关仍是 `sstatus.SIE`。
#[inline]
pub fn set_sie(bits: usize) {
    csrs!("sie", bits);
}

/// 读 `sip` (等待处理的中断)。读某些实现上会清掉其中部分位, 只在调试用。
#[inline]
pub fn read_sip() -> usize {
    csrr!("sip")
}

// ===========================================================================
// scause —— 陷入原因
// ===========================================================================
// scause 最高位是"中断/异常", 其余低位是编号。判读必须先看最高位,
// 否则会把"时钟中断 (5)"和"读缺页 (5)"搞混。

/// 最高位: 1 表示中断。
pub const SCAUSE_INTERRUPT: usize = 1 << (usize::BITS as usize - 1);
/// 取出低位的编号。
pub const SCAUSE_CODE_MASK: usize = !SCAUSE_INTERRUPT;

/// 中断编号: S-mode 软件中断 (核间中断)。
pub const IRQ_S_SOFTWARE: usize = 1;
/// 中断编号: S-mode 时钟中断。
pub const IRQ_S_TIMER: usize = 5;
/// 中断编号: S-mode 外部中断 (来自 PLIC)。
pub const IRQ_S_EXTERNAL: usize = 9;

/// 异常编号: 指令地址未对齐。
pub const EXC_INST_MISALIGNED: usize = 0;
/// 异常编号: 取指缺页。
pub const EXC_INST_PAGE_FAULT: usize = 12;
/// 异常编号: 读缺页。
pub const EXC_LOAD_PAGE_FAULT: usize = 13;
/// 异常编号: 写缺页。
pub const EXC_STORE_PAGE_FAULT: usize = 15;
/// 异常编号: 从 U-mode 发起的 `ecall` (系统调用)。
pub const EXC_ECALL_FROM_U: usize = 8;
/// 异常编号: 从 S-mode 发起的 `ecall`。
///
/// 内核自己 `ecall` 调 SBI 时, 若参数或扩展号错, 固件会把它变成一次
/// S-mode 异常回抛 —— 看到它就说明"SBI 调用被拒绝了"。
pub const EXC_ECALL_FROM_S: usize = 9;
/// 异常编号: 非法指令。
pub const EXC_ILLEGAL_INST: usize = 2;

/// 读 `scause`。
#[inline]
pub fn read_scause() -> usize {
    csrr!("scause")
}

/// scause 是否表示一次中断。
#[inline]
pub fn scause_is_interrupt(scause: usize) -> bool {
    scause & SCAUSE_INTERRUPT != 0
}

/// 从 scause 取出编号 (去掉最高位)。
#[inline]
pub fn scause_code(scause: usize) -> usize {
    scause & SCAUSE_CODE_MASK
}

// ===========================================================================
// sepc / stval / stvec
// ===========================================================================

/// 读 `sepc` (触发 trap 的 PC)。
#[inline]
pub fn read_sepc() -> usize {
    csrr!("sepc")
}

/// 写 `sepc`。
///
/// # Safety
/// 改错 `sepc` 的后果是 `sret` 后跳到任意地址。系统调用返回前**必须**
/// 把它 +4 (见 `trap::advance_sepc_for_ecall`), 否则同一 `ecall` 无限重复。
#[inline]
pub unsafe fn write_sepc(v: usize) {
    csrw!("sepc", v);
}

/// 读 `stval` (trap 的附加信息: 缺页地址、非法指令编码)。
#[inline]
pub fn read_stval() -> usize {
    csrr!("stval")
}

/// 写 `stvec` (trap 向量入口)。
///
/// # Safety
/// `stvec` 低 2 位是模式位 (0 = Direct, 1 = Vectored)。本内核只用
/// Direct, 所以这里强制清低 2 位。
#[inline]
pub unsafe fn write_stvec(v: usize) {
    // 强制对齐: 地址低 2 位必须是 0 (模式位), 否则硬件会把地址位当模式位。
    csrw!("stvec", v & !0b11);
}

/// 读 `stvec`。
#[inline]
pub fn read_stvec() -> usize {
    csrr!("stvec")
}

// ===========================================================================
// satp —— 页表基址寄存器
// ===========================================================================
// satp (RV64): bit 63:60 MODE (8 = Sv39), 59:44 ASID, 43:0 PPN (根页表
// **物理页号**, 不是物理地址)。直接写物理地址会静默卡死。所以本模块只
// 提供 `activate_page_table(root_pa)` (在 mm.rs), 让移位在类型层面可控。

/// Sv39 的 MODE 字段值。
pub const SATP_MODE_SV39: usize = 8;

/// 写 `satp`, 参数是**已编码好的完整值**。
///
/// 低层入口, 只应由 [`mm::activate_page_table`] 调用。
///
/// # Safety
/// 调用者必须保证: (1) 传进来的是合法 satp 编码; (2) 该页表已为当前
/// 代码和栈建立映射 (否则下一条指令就缺页)。
#[inline]
pub(crate) unsafe fn write_satp_raw(v: usize) {
    csrw!("satp", v);
    // 切页表后必须刷新 TLB, 否则 TLB 里的旧映射会产生**随机**地址翻译错误。
    // 放在这里而不是让调用者记得, 是因为"记得写"不可靠。
    // SAFETY: `sfence.vma` 在任何特权级都合法, 不依赖内存状态。
    unsafe { tlb_flush_all() };
}

/// 读 `satp`。
#[inline]
pub fn read_satp() -> usize {
    csrr!("satp")
}

/// 刷新全部 TLB。
///
/// # Safety
/// 指令本身安全, 但语义上要求调用者已做完页表修改并配上内存屏障。
#[inline]
pub unsafe fn tlb_flush_all() {
    // SAFETY: 由调用者保证页表修改之后需要刷新。
    unsafe {
        ::core::arch::asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
    }
}

/// 刷新某个虚拟地址对应的 TLB 条目。
///
/// # Safety
/// 同 [`tlb_flush_all`]。
#[inline]
pub unsafe fn tlb_flush_page(va: usize) {
    // SAFETY: 由调用者保证。
    unsafe {
        ::core::arch::asm!("sfence.vma {}, zero", in(reg) va, options(nostack, preserves_flags));
    }
}

// ===========================================================================
// 内存屏障
// ===========================================================================
// RISC-V 是弱内存序架构。驱动里"先写描述符, 再写 doorbell"的顺序必须
// 显式保持, 否则设备可能读到未写完的数据 —— "在 QEMU 能跑、真机偶发失败"
// 的经典来源。

/// 全屏障。
#[inline]
pub fn fence() {
    unsafe { ::core::arch::asm!("fence", options(nostack, preserves_flags)) };
}

/// 写-写屏障。
#[inline]
pub fn fence_w() {
    unsafe { ::core::arch::asm!("fence w, w", options(nostack, preserves_flags)) };
}

/// 读-读屏障。
#[inline]
pub fn fence_r() {
    unsafe { ::core::arch::asm!("fence r, r", options(nostack, preserves_flags)) };
}

/// 设备 I/O 屏障: 保证普通内存写对设备可见, 且不被重排到后续 MMIO 写之后。
///
/// 裸 `fence` 在某些实现上不保证普通内存写与设备 MMIO 写之间的顺序 ——
/// 驱动"写描述符 (内存) -> 写 doorbell (MMIO)"若被重排, 设备会处理
/// 不存在的请求。缺它表现为时序不稳定 ("加一句打印就通过")。
#[inline]
pub fn fence_io() {
    // `iorw, iorw` = 对 I/O 与内存的读、写都排序。
    unsafe { ::core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags)) };
}

/// 同步指令缓存与数据缓存。
///
/// 写完代码再跳过去执行 (JIT、动态加载、把用户程序读进内存后跳过去) 前,
/// 不带它取指可能读到旧 I-cache 内容。
#[inline]
pub fn fence_i() {
    unsafe { ::core::arch::asm!("fence.i", options(nostack, preserves_flags)) };
}

// ===========================================================================
// 编译期自检
// ===========================================================================
// 防"抄错位号" —— 位号抄错往往不是崩溃而是"某个功能随机不工作"。
const _: () = {
    assert!(SSTATUS_SIE == 0x2);
    assert!(SSTATUS_SPIE == 0x20);
    assert!(SSTATUS_SPP == 0x100); // 第 8 位, 与汇编里的 SSTATUS_SPP_BIT 对应
    assert!(SSTATUS_SUM == 0x40000);
    assert!(SIE_STIE == 0x20);
    assert!(SIE_SEIE == 0x200);
    assert!(SATP_MODE_SV39 == 8);
    assert!(IRQ_S_TIMER == 5);
    assert!(IRQ_S_EXTERNAL == 9);
    assert!(EXC_ECALL_FROM_U == 8);
};
